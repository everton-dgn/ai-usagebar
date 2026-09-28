//! LaunchAgent for "start at login". Writing the plist is enough: launchd
//! loads `~/Library/LaunchAgents` at the next login. We do not `launchctl
//! load` here — that would spawn a second copy of the already-running tray.
//!
//! Only a plist byte-identical to the one this module writes is ever
//! rewritten; anything else under our label is reported and left alone.
//! Before an overwrite or a removal the current file is copied, never
//! overwriting an earlier copy, under [`BACKUP_ROOT`]; turning the item off
//! then moves it to the Trash, so every change can be undone.

use std::fs;
use std::io::{ErrorKind, Write as _};
use std::path::{Component, Path, PathBuf};

const LABEL: &str = "com.akitaonrails.ai-usagebar-tray";

/// Permanent copies of the plist taken before it is overwritten or trashed,
/// under `<stamp>/<absolute path of the plist>`.
const BACKUP_ROOT: &str = "/tmp/claude-backups";

/// Everything in the plist after the escaped program path.
const PLIST_TAIL: &str = r#"</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#;

/// Where a copy that must never become the login item may be running from:
/// build output, a mounted disk image or Gatekeeper's App Translocation.
const TRANSIENT_ROOTS: [&str; 5] = [
    "/tmp",
    "/private/tmp",
    "/var/folders",
    "/private/var/folders",
    "/Volumes",
];

/// What sits at our LaunchAgent path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Absent,
    /// Our plist, opening this copy of the app.
    Current,
    /// Our plist, opening another executable (an older install location).
    Legacy {
        program: PathBuf,
    },
    /// Something under our label this module did not write.
    Unrecognized(String),
}

impl Entry {
    /// Whether launchd will start something under our label at login. A
    /// legacy path still counts: the user turned the item on.
    pub fn is_enabled(&self) -> bool {
        !matches!(self, Self::Absent)
    }
}

/// Result of [`reconcile`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reconciled {
    /// Start at login is off; nothing was written.
    Disabled,
    /// The plist already opens this copy.
    Current,
    /// The plist opened `from` and now opens this copy.
    Migrated { from: PathBuf },
    /// A legacy plist was kept because moving it here would be unsafe.
    Skipped(String),
}

pub fn is_enabled() -> bool {
    match status() {
        Ok(entry) => entry.is_enabled(),
        // Unreadable is not the same as off: report what launchd will see.
        Err(_) => plist_path().is_some_and(|path| fs::symlink_metadata(path).is_ok()),
    }
}

/// What the LaunchAgent opens, compared with this running copy.
pub fn status() -> Result<Entry, String> {
    inspect_at(&plist_path_or_err()?, &exe_path()?)
}

/// Points an enabled login item left at an older install location to this
/// copy. Never turns the item on, never touches a plist it did not write and
/// only moves it to an installed app bundle. Idempotent; call once at launch,
/// away from the window thread.
pub fn reconcile() -> Result<Reconciled, String> {
    reconcile_at(&plist_path_or_err()?, &exe_path()?, &system_io())
}

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let plist = plist_path_or_err()?;
    if enabled {
        enable_at(&plist, &exe_path()?, &system_io())
    } else {
        disable_at(&plist, &system_io())
    }
}

fn plist_path() -> Option<PathBuf> {
    crate::cache::home_dir().ok().map(|home| {
        home.join("Library/LaunchAgents")
            .join(format!("{LABEL}.plist"))
    })
}

fn plist_path_or_err() -> Result<PathBuf, String> {
    plist_path().ok_or_else(|| "Could not find the home folder".to_string())
}

fn exe_path() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|error| error.to_string())
}

/// Filesystem side effects that tests replace: where backups go, the
/// `AAAAMMDD_HHMMSS` folder they land in and how a file reaches the Trash.
struct Io<'a> {
    backup_root: PathBuf,
    stamp: String,
    trash: &'a dyn Fn(&Path) -> Result<(), String>,
}

/// Same-second backups get `_1`, `_2`… rather than replacing each other.
const MAX_BACKUP_ATTEMPTS: usize = 100;

fn system_io() -> Io<'static> {
    Io {
        backup_root: PathBuf::from(BACKUP_ROOT),
        stamp: chrono::Local::now().format("%Y%m%d_%H%M%S").to_string(),
        trash: &trash_item,
    }
}

fn trash_item(path: &Path) -> Result<(), String> {
    use objc2_foundation::{NSFileManager, NSString, NSURL};

    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, None)
        .map_err(|error| {
            format!(
                "Could not move the login item to the Trash: {}",
                crate::display::sanitize_untrusted_line(&error.localizedDescription().to_string())
            )
        })
}

fn inspect_at(plist: &Path, exe: &Path) -> Result<Entry, String> {
    let meta = match fs::symlink_metadata(plist) {
        Ok(meta) => meta,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Entry::Absent),
        Err(error) => return Err(io_failure("read", &error)),
    };
    if meta.file_type().is_symlink() {
        return Ok(Entry::Unrecognized(
            "The login item is a link this app did not create".into(),
        ));
    }
    if !meta.is_file() {
        return Ok(Entry::Unrecognized("The login item is not a file".into()));
    }
    let bytes = fs::read(plist).map_err(|error| io_failure("read", &error))?;
    let Some(program) = String::from_utf8(bytes)
        .ok()
        .and_then(|text| program_in(&text))
    else {
        return Ok(Entry::Unrecognized(
            "The login item was not written by this app".into(),
        ));
    };
    if same_executable(&program, exe) {
        Ok(Entry::Current)
    } else {
        Ok(Entry::Legacy { program })
    }
}

fn reconcile_at(plist: &Path, exe: &Path, io: &Io<'_>) -> Result<Reconciled, String> {
    match inspect_at(plist, exe)? {
        Entry::Absent => Ok(Reconciled::Disabled),
        Entry::Current => Ok(Reconciled::Current),
        Entry::Unrecognized(reason) => Err(reason),
        Entry::Legacy { program } => {
            if let Some(reason) = migration_blocker(exe) {
                return Ok(Reconciled::Skipped(reason.into()));
            }
            // Two installed bundles: keep the one the user chose at login.
            if in_app_bundle(&program) && program.is_file() {
                return Ok(Reconciled::Skipped(
                    "The login item opens another copy of the app".into(),
                ));
            }
            replace(plist, exe, io)?;
            Ok(Reconciled::Migrated { from: program })
        }
    }
}

fn enable_at(plist: &Path, exe: &Path, io: &Io<'_>) -> Result<(), String> {
    match inspect_at(plist, exe)? {
        Entry::Current => Ok(()),
        Entry::Absent => write_plist(plist, exe),
        Entry::Legacy { .. } => replace(plist, exe, io),
        Entry::Unrecognized(reason) => Err(reason),
    }
}

fn disable_at(plist: &Path, io: &Io<'_>) -> Result<(), String> {
    match fs::symlink_metadata(plist) {
        Ok(_) => {
            back_up(plist, io)?;
            (io.trash)(plist)
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_failure("read", &error)),
    }
}

/// Backs up the current plist, then swaps in the new one atomically. A
/// failed backup leaves the old plist untouched.
fn replace(plist: &Path, exe: &Path, io: &Io<'_>) -> Result<(), String> {
    back_up(plist, io)?;
    write_plist(plist, exe)
}

/// Copies the plist (or the link itself) to
/// `<backup_root>/<stamp>/<absolute plist path>`. Never replaces an earlier
/// backup and never removes one.
fn back_up(plist: &Path, io: &Io<'_>) -> Result<PathBuf, String> {
    let meta = fs::symlink_metadata(plist).map_err(|error| io_failure("back up", &error))?;
    let is_link = meta.file_type().is_symlink();
    if !is_link && !meta.is_file() {
        return Err("The login item is not a file".into());
    }
    let relative = plist.strip_prefix("/").unwrap_or(plist);
    for attempt in 0..MAX_BACKUP_ATTEMPTS {
        let folder = match attempt {
            0 => io.stamp.clone(),
            n => format!("{}_{n}", io.stamp),
        };
        let target = io.backup_root.join(folder).join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|error| io_failure("back up", &error))?;
        }
        let copied = if is_link {
            fs::read_link(plist).and_then(|link| std::os::unix::fs::symlink(link, &target))
        } else {
            copy_new(plist, &target)
        };
        match copied {
            Ok(()) => return Ok(target),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(io_failure("back up", &error)),
        }
    }
    Err("Could not back up the login item: too many backups this second".into())
}

/// `fs::copy` that refuses to replace an existing file.
fn copy_new(from: &Path, to: &Path) -> std::io::Result<()> {
    let bytes = fs::read(from)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)?;
    file.write_all(&bytes)?;
    file.sync_all()
}

fn write_plist(plist: &Path, exe: &Path) -> Result<(), String> {
    crate::cache::atomic_write(plist, plist_body(&exe.to_string_lossy()).as_bytes())
        .map_err(|error| format!("Could not save the login item: {error}"))
}

fn plist_head() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>"#
    )
}

fn plist_body(program: &str) -> String {
    format!("{}{}{PLIST_TAIL}", plist_head(), xml_escape(program))
}

/// The program of a plist this module wrote, or `None` for any other file.
/// Regenerating and comparing bytes rejects extra keys, another label and
/// any escaping this module would not produce.
fn program_in(text: &str) -> Option<PathBuf> {
    let escaped = text
        .strip_prefix(plist_head().as_str())?
        .strip_suffix(PLIST_TAIL)?;
    let program = xml_unescape(escaped);
    (Path::new(&program).is_absolute() && plist_body(&program) == text)
        .then(|| PathBuf::from(program))
}

fn same_executable(program: &Path, exe: &Path) -> bool {
    if program == exe {
        return true;
    }
    matches!(
        (fs::canonicalize(program), fs::canonicalize(exe)),
        (Ok(a), Ok(b)) if a == b
    )
}

fn in_app_bundle(exe: &Path) -> bool {
    let macos = exe.parent();
    let contents = macos.and_then(Path::parent);
    let bundle = contents.and_then(Path::parent);
    macos
        .and_then(Path::file_name)
        .is_some_and(|name| name == "MacOS")
        && contents
            .and_then(Path::file_name)
            .is_some_and(|name| name == "Contents")
        && bundle
            .and_then(Path::extension)
            .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
}

fn migration_blocker(exe: &Path) -> Option<&'static str> {
    if !in_app_bundle(exe) {
        return Some("This copy is not running from an app bundle");
    }
    let transient = TRANSIENT_ROOTS.iter().any(|root| exe.starts_with(root))
        || exe
            .components()
            .any(|part| part == Component::Normal("AppTranslocation".as_ref()));
    transient.then_some("This copy is running from a temporary location")
}

fn io_failure(action: &str, error: &std::io::Error) -> String {
    format!("Could not {action} the login item: {error}")
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn xml_unescape(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&gt;", ">")
        .replace("&lt;", "<")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    use super::*;

    /// Bytes the released builds wrote, spelled out independently of
    /// `plist_body` so a template drift cannot hide behind itself.
    fn released_plist(program: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.akitaonrails.ai-usagebar-tray</string>
    <key>ProgramArguments</key>
    <array>
        <string>{program}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#
        )
    }

    const BUNDLE_EXE: &str = "/Applications/AI Usage Fixture.app/Contents/MacOS/ai-usagebar-tray";
    const LEGACY_EXE: &str = "/Users/fixture/.cargo/bin/ai-usagebar-tray";

    const STAMP: &str = "20260928_120000";

    /// A LaunchAgents dir, a backup root, a scratch dir and a fake Trash,
    /// all temporary.
    struct Sandbox {
        _root: tempfile::TempDir,
        plist: PathBuf,
        scratch: PathBuf,
        backup_root: PathBuf,
        trash_dir: PathBuf,
        trashed: RefCell<Vec<PathBuf>>,
        refuse_trash: bool,
    }

    impl Sandbox {
        fn new() -> Self {
            let root = tempfile::tempdir().expect("tempdir");
            let agents = root.path().join("LaunchAgents");
            let scratch = root.path().join("scratch");
            let trash_dir = root.path().join("Trash");
            for dir in [&agents, &scratch, &trash_dir] {
                fs::create_dir_all(dir).expect("mkdir");
            }
            Self {
                plist: agents.join(format!("{LABEL}.plist")),
                backup_root: root.path().join("claude-backups"),
                scratch,
                trash_dir,
                trashed: RefCell::new(Vec::new()),
                refuse_trash: false,
                _root: root,
            }
        }

        fn trash(&self, path: &Path) -> Result<(), String> {
            if self.refuse_trash {
                return Err("Trash unavailable".into());
            }
            let n = self.trashed.borrow().len();
            let target = self.trash_dir.join(format!(
                "{n}-{}",
                path.file_name().unwrap().to_string_lossy()
            ));
            fs::rename(path, &target).map_err(|error| error.to_string())?;
            self.trashed.borrow_mut().push(target);
            Ok(())
        }

        fn run<T>(&self, f: impl FnOnce(&Io<'_>) -> T) -> T {
            let trash = |path: &Path| self.trash(path);
            f(&Io {
                backup_root: self.backup_root.clone(),
                stamp: STAMP.into(),
                trash: &trash,
            })
        }

        fn write(&self, body: &str) {
            fs::write(&self.plist, body).expect("seed plist");
        }

        fn read(&self) -> String {
            fs::read_to_string(&self.plist).expect("read plist")
        }

        fn trashed_bodies(&self) -> Vec<String> {
            self.trashed
                .borrow()
                .iter()
                .map(|path| fs::read_to_string(path).expect("read trashed"))
                .collect()
        }

        /// Where the first backup of this run lands.
        fn backup_path(&self) -> PathBuf {
            self.backup_root
                .join(STAMP)
                .join(self.plist.strip_prefix("/").expect("absolute plist"))
        }

        /// Every backup body, oldest first.
        fn backup_bodies(&self) -> Vec<String> {
            let mut found = Vec::new();
            let mut pending = vec![self.backup_root.clone()];
            while let Some(dir) = pending.pop() {
                let Ok(entries) = fs::read_dir(&dir) else {
                    continue;
                };
                for entry in entries {
                    let path = entry.expect("entry").path();
                    if path.is_dir() {
                        pending.push(path);
                    } else {
                        found.push(path);
                    }
                }
            }
            found.sort();
            found
                .iter()
                .map(|path| fs::read_to_string(path).expect("read backup"))
                .collect()
        }

        /// A file where the backup root should be, so no backup can be made.
        fn block_backups(&self) {
            fs::write(&self.backup_root, b"").expect("block backups");
        }
    }

    #[test]
    fn the_template_is_the_one_released_builds_wrote() {
        assert_eq!(plist_body(BUNDLE_EXE), released_plist(BUNDLE_EXE));
        assert_eq!(
            plist_body("/Apps/A&B <x>.app/Contents/MacOS/t\"q"),
            released_plist("/Apps/A&amp;B &lt;x&gt;.app/Contents/MacOS/t&quot;q")
        );
    }

    #[test]
    fn a_missing_plist_is_disabled_and_reconcile_never_creates_one() {
        let sandbox = Sandbox::new();
        let exe = Path::new(BUNDLE_EXE);
        assert_eq!(inspect_at(&sandbox.plist, exe), Ok(Entry::Absent));
        assert!(!Entry::Absent.is_enabled());
        assert_eq!(
            sandbox.run(|io| reconcile_at(&sandbox.plist, exe, io)),
            Ok(Reconciled::Disabled)
        );
        assert!(!sandbox.plist.exists());
        assert!(sandbox.trashed.borrow().is_empty());
    }

    #[test]
    fn a_plist_for_this_copy_is_current_and_enabled() {
        let sandbox = Sandbox::new();
        sandbox.write(&released_plist(BUNDLE_EXE));
        let entry = inspect_at(&sandbox.plist, Path::new(BUNDLE_EXE)).expect("inspect");
        assert_eq!(entry, Entry::Current);
        assert!(entry.is_enabled());
    }

    #[test]
    fn a_legacy_path_stays_enabled() {
        let sandbox = Sandbox::new();
        sandbox.write(&released_plist(LEGACY_EXE));
        let entry = inspect_at(&sandbox.plist, Path::new(BUNDLE_EXE)).expect("inspect");
        assert_eq!(
            entry,
            Entry::Legacy {
                program: PathBuf::from(LEGACY_EXE)
            }
        );
        assert!(entry.is_enabled());
    }

    #[test]
    fn reconcile_moves_a_legacy_path_to_this_copy_after_a_permanent_backup() {
        let sandbox = Sandbox::new();
        let legacy = released_plist(LEGACY_EXE);
        sandbox.write(&legacy);
        let exe = Path::new(BUNDLE_EXE);

        let outcome = sandbox.run(|io| reconcile_at(&sandbox.plist, exe, io));

        assert_eq!(
            outcome,
            Ok(Reconciled::Migrated {
                from: PathBuf::from(LEGACY_EXE)
            })
        );
        assert_eq!(sandbox.read(), released_plist(BUNDLE_EXE));
        assert_eq!(
            fs::read_to_string(sandbox.backup_path()).expect("backup"),
            legacy
        );
        assert_eq!(sandbox.backup_bodies(), vec![legacy]);
        assert!(
            sandbox.trashed.borrow().is_empty(),
            "an overwrite trashes nothing"
        );
        assert!(fs::read_dir(&sandbox.scratch).unwrap().next().is_none());
        assert_eq!(inspect_at(&sandbox.plist, exe), Ok(Entry::Current));
    }

    #[test]
    fn reconcile_is_idempotent() {
        let sandbox = Sandbox::new();
        sandbox.write(&released_plist(LEGACY_EXE));
        let exe = Path::new(BUNDLE_EXE);
        sandbox
            .run(|io| reconcile_at(&sandbox.plist, exe, io))
            .expect("first");
        let after_first = sandbox.read();

        for _ in 0..2 {
            assert_eq!(
                sandbox.run(|io| reconcile_at(&sandbox.plist, exe, io)),
                Ok(Reconciled::Current)
            );
        }
        assert_eq!(sandbox.read(), after_first);
        assert_eq!(
            sandbox.backup_bodies().len(),
            1,
            "one backup, not one per launch"
        );
        assert!(sandbox.trashed.borrow().is_empty());
    }

    #[test]
    fn a_path_with_markup_round_trips() {
        let sandbox = Sandbox::new();
        let legacy = "/Users/fixture/A&B <bin>/ai-usagebar-tray";
        sandbox.write(&plist_body(legacy));
        assert_eq!(
            inspect_at(&sandbox.plist, Path::new(BUNDLE_EXE)),
            Ok(Entry::Legacy {
                program: PathBuf::from(legacy)
            })
        );
    }

    #[test]
    fn reconcile_skips_when_this_copy_is_not_an_installed_bundle() {
        for exe in [
            "/private/tmp/build/AI Usage.app/Contents/MacOS/ai-usagebar-tray",
            "/var/folders/xy/T/AppTranslocation/ABC/d/AI Usage.app/Contents/MacOS/ai-usagebar-tray",
            "/Volumes/AI Usage/AI Usage.app/Contents/MacOS/ai-usagebar-tray",
            "/Users/fixture/www/ai-usagebar/target/release/ai-usagebar-tray",
        ] {
            let sandbox = Sandbox::new();
            let legacy = released_plist(LEGACY_EXE);
            sandbox.write(&legacy);
            let outcome = sandbox.run(|io| reconcile_at(&sandbox.plist, Path::new(exe), io));
            assert!(
                matches!(outcome, Ok(Reconciled::Skipped(_))),
                "{exe}: {outcome:?}"
            );
            assert_eq!(sandbox.read(), legacy, "{exe}");
            assert!(sandbox.trashed.borrow().is_empty(), "{exe}");
            assert!(sandbox.backup_bodies().is_empty(), "{exe}");
        }
    }

    #[test]
    fn reconcile_keeps_a_login_item_that_opens_another_existing_bundle() {
        let sandbox = Sandbox::new();
        let other = sandbox
            .scratch
            .join("Other.app/Contents/MacOS/ai-usagebar-tray");
        fs::create_dir_all(other.parent().unwrap()).expect("mkdir");
        fs::write(&other, b"").expect("fake exe");
        let body = plist_body(&other.to_string_lossy());
        sandbox.write(&body);

        let outcome = sandbox.run(|io| reconcile_at(&sandbox.plist, Path::new(BUNDLE_EXE), io));

        assert!(matches!(outcome, Ok(Reconciled::Skipped(_))), "{outcome:?}");
        assert_eq!(sandbox.read(), body);
    }

    #[test]
    fn a_malformed_plist_is_reported_and_left_alone() {
        let sandbox = Sandbox::new();
        let broken = "<?xml version=\"1.0\"?><plist><dict><key>Label</key>";
        sandbox.write(broken);
        let exe = Path::new(BUNDLE_EXE);

        let entry = inspect_at(&sandbox.plist, exe).expect("inspect");
        assert!(matches!(entry, Entry::Unrecognized(_)), "{entry:?}");
        assert!(
            entry.is_enabled(),
            "something under our label still loads at login"
        );
        assert!(
            sandbox
                .run(|io| reconcile_at(&sandbox.plist, exe, io))
                .is_err()
        );
        assert!(
            sandbox
                .run(|io| enable_at(&sandbox.plist, exe, io))
                .is_err()
        );
        assert_eq!(sandbox.read(), broken);
        assert!(sandbox.trashed.borrow().is_empty());
    }

    #[test]
    fn another_job_or_an_edited_plist_is_left_alone() {
        let exe = Path::new(BUNDLE_EXE);
        for body in [
            released_plist(LEGACY_EXE).replace(LABEL, "com.example.other-app"),
            released_plist(LEGACY_EXE).replace(
                "    <key>RunAtLoad</key>",
                "    <key>KeepAlive</key>\n    <true/>\n    <key>RunAtLoad</key>",
            ),
            released_plist("relative/ai-usagebar-tray"),
        ] {
            let sandbox = Sandbox::new();
            sandbox.write(&body);
            assert!(matches!(
                inspect_at(&sandbox.plist, exe),
                Ok(Entry::Unrecognized(_))
            ));
            assert!(
                sandbox
                    .run(|io| reconcile_at(&sandbox.plist, exe, io))
                    .is_err()
            );
            assert_eq!(sandbox.read(), body);
        }
    }

    #[test]
    fn a_symlink_or_directory_is_not_rewritten() {
        let exe = Path::new(BUNDLE_EXE);
        let sandbox = Sandbox::new();
        fs::create_dir(&sandbox.plist).expect("dir in place of plist");
        assert!(matches!(
            inspect_at(&sandbox.plist, exe),
            Ok(Entry::Unrecognized(_))
        ));
        assert!(
            sandbox
                .run(|io| reconcile_at(&sandbox.plist, exe, io))
                .is_err()
        );
        assert!(sandbox.plist.is_dir());

        let sandbox = Sandbox::new();
        let target = sandbox.scratch.join("real.plist");
        fs::write(&target, released_plist(LEGACY_EXE)).expect("target");
        std::os::unix::fs::symlink(&target, &sandbox.plist).expect("symlink");
        assert!(matches!(
            inspect_at(&sandbox.plist, exe),
            Ok(Entry::Unrecognized(_))
        ));
        assert!(
            sandbox
                .run(|io| reconcile_at(&sandbox.plist, exe, io))
                .is_err()
        );
        assert!(
            fs::symlink_metadata(&sandbox.plist)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn an_unreadable_plist_is_an_error_not_disabled() {
        let sandbox = Sandbox::new();
        sandbox.write(&released_plist(LEGACY_EXE));
        fs::set_permissions(&sandbox.plist, fs::Permissions::from_mode(0o000)).expect("chmod");
        let exe = Path::new(BUNDLE_EXE);
        let inspected = inspect_at(&sandbox.plist, exe);
        let reconciled = sandbox.run(|io| reconcile_at(&sandbox.plist, exe, io));
        fs::set_permissions(&sandbox.plist, fs::Permissions::from_mode(0o644)).expect("chmod back");
        assert!(inspected.is_err(), "{inspected:?}");
        assert!(reconciled.is_err(), "{reconciled:?}");
        assert_eq!(sandbox.read(), released_plist(LEGACY_EXE));
    }

    #[test]
    fn a_failed_backup_leaves_the_legacy_plist_in_place() {
        let sandbox = Sandbox::new();
        sandbox.block_backups();
        let legacy = released_plist(LEGACY_EXE);
        sandbox.write(&legacy);

        let outcome = sandbox.run(|io| reconcile_at(&sandbox.plist, Path::new(BUNDLE_EXE), io));

        assert!(outcome.is_err(), "{outcome:?}");
        assert_eq!(sandbox.read(), legacy);
        assert!(sandbox.trashed.borrow().is_empty());
    }

    #[test]
    fn backups_in_the_same_second_do_not_overwrite_each_other() {
        let sandbox = Sandbox::new();
        let exe = Path::new(BUNDLE_EXE);
        let first = released_plist(LEGACY_EXE);
        let second = released_plist("/Users/fixture/other/ai-usagebar-tray");
        sandbox.write(&first);
        sandbox
            .run(|io| enable_at(&sandbox.plist, exe, io))
            .expect("first");
        sandbox.write(&second);
        sandbox
            .run(|io| enable_at(&sandbox.plist, exe, io))
            .expect("second");
        let mut bodies = sandbox.backup_bodies();
        bodies.sort();
        let mut expected = vec![first, second];
        expected.sort();
        assert_eq!(bodies, expected);
    }

    #[test]
    fn enabling_writes_this_copy_and_backs_up_a_legacy_plist() {
        let sandbox = Sandbox::new();
        let exe = Path::new(BUNDLE_EXE);
        sandbox
            .run(|io| enable_at(&sandbox.plist, exe, io))
            .expect("enable");
        assert_eq!(sandbox.read(), released_plist(BUNDLE_EXE));
        assert!(sandbox.trashed.borrow().is_empty());
        assert!(sandbox.backup_bodies().is_empty(), "nothing to back up");

        let sandbox = Sandbox::new();
        let legacy = released_plist(LEGACY_EXE);
        sandbox.write(&legacy);
        sandbox
            .run(|io| enable_at(&sandbox.plist, exe, io))
            .expect("enable");
        assert_eq!(sandbox.read(), released_plist(BUNDLE_EXE));
        assert_eq!(sandbox.backup_bodies(), vec![legacy]);
        assert!(sandbox.trashed.borrow().is_empty());
    }

    #[test]
    fn disabling_moves_the_plist_to_the_trash() {
        let sandbox = Sandbox::new();
        let body = released_plist(BUNDLE_EXE);
        sandbox.write(&body);
        sandbox
            .run(|io| disable_at(&sandbox.plist, io))
            .expect("disable");
        assert!(!sandbox.plist.exists());
        assert_eq!(sandbox.trashed_bodies(), vec![body.clone()]);
        assert_eq!(sandbox.backup_bodies(), vec![body]);
        assert_eq!(
            inspect_at(&sandbox.plist, Path::new(BUNDLE_EXE)),
            Ok(Entry::Absent)
        );

        sandbox
            .run(|io| disable_at(&sandbox.plist, io))
            .expect("already off");
        assert_eq!(sandbox.trashed.borrow().len(), 1);
        assert_eq!(sandbox.backup_bodies().len(), 1);
    }

    #[test]
    fn a_failed_disable_is_reported() {
        let mut sandbox = Sandbox::new();
        sandbox.refuse_trash = true;
        sandbox.write(&released_plist(BUNDLE_EXE));
        assert!(sandbox.run(|io| disable_at(&sandbox.plist, io)).is_err());
        assert!(sandbox.plist.exists());
    }

    #[test]
    fn disabling_without_a_backup_is_refused() {
        let sandbox = Sandbox::new();
        sandbox.block_backups();
        sandbox.write(&released_plist(BUNDLE_EXE));
        assert!(sandbox.run(|io| disable_at(&sandbox.plist, io)).is_err());
        assert!(sandbox.plist.exists());
        assert!(sandbox.trashed.borrow().is_empty());

        let sandbox = Sandbox::new();
        fs::create_dir(&sandbox.plist).expect("dir in place of plist");
        assert!(sandbox.run(|io| disable_at(&sandbox.plist, io)).is_err());
        assert!(sandbox.plist.is_dir());
        assert!(sandbox.trashed.borrow().is_empty());
    }
}
