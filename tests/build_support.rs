//! Fail-closed behavior of the tray frontend build that `build.rs` runs.
//!
//! Hermetic: every frontend, `OUT_DIR` and builder is a fixture under a fresh
//! temp dir that is kept, never removed. Nothing here resolves a real Node,
//! reads `$HOME`, or touches `frontend/`.

#[path = "../src/build_support.rs"]
mod build_support;

use std::fs;
use std::path::{Path, PathBuf};

use build_support::{BuildError, REQUIRED_ASSETS, build_popover, build_popover_with};

fn sandbox(label: &str) -> PathBuf {
    tempfile::Builder::new()
        .prefix(&format!("aiub-build-support-{label}-"))
        .tempdir()
        .expect("temp dir")
        .keep()
}

struct Fixture {
    frontend: PathBuf,
    out_dir: PathBuf,
}

impl Fixture {
    /// A frontend whose source `dist/` already holds a complete, valid build:
    /// the stale output a fail-closed build must never mistake for its own.
    fn new(label: &str, installed: bool) -> Self {
        let root = sandbox(label);
        let frontend = root.join("popover");
        fs::create_dir_all(frontend.join("dist")).unwrap();
        fs::write(frontend.join("package-lock.json"), "{}\n").unwrap();
        for name in REQUIRED_ASSETS {
            fs::write(frontend.join("dist").join(name), format!("stale {name}\n")).unwrap();
        }
        if installed {
            fs::create_dir_all(frontend.join("node_modules/vite/bin")).unwrap();
            fs::write(vite_cli(&frontend), "#!/usr/bin/env node\n").unwrap();
        }
        let out_dir = root.join("out");
        fs::create_dir_all(&out_dir).unwrap();
        Self { frontend, out_dir }
    }

    fn staged(&self) -> PathBuf {
        self.out_dir.join("popover")
    }

    /// A previous successful build already staged, as `OUT_DIR` keeps it.
    fn seed_previous_stage(&self) {
        fs::create_dir_all(self.staged()).unwrap();
        for name in REQUIRED_ASSETS {
            fs::write(self.staged().join(name), format!("previous {name}\n")).unwrap();
        }
    }

    fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
        REQUIRED_ASSETS
            .iter()
            .map(|name| {
                (
                    name.to_string(),
                    fs::read(dir.join(name)).unwrap_or_default(),
                )
            })
            .collect()
    }
}

fn vite_cli(frontend: &Path) -> PathBuf {
    frontend.join("node_modules/vite/bin/vite.js")
}

fn write_assets(dir: &Path) {
    for name in REQUIRED_ASSETS {
        fs::write(dir.join(name), format!("fresh {name}\n")).unwrap();
    }
}

#[test]
fn missing_frontend_dependencies_fail_without_invoking_the_builder() {
    let fx = Fixture::new("no-deps", false);
    let mut invoked = false;
    let result = build_popover_with(&fx.frontend, &fx.out_dir, |_| {
        invoked = true;
        Ok(())
    });
    assert!(
        matches!(result, Err(BuildError::MissingDependencies { .. })),
        "{result:?}"
    );
    assert!(!invoked, "the build must never install or run without deps");
    assert!(!fx.staged().exists(), "nothing may be staged");
    let message = result.unwrap_err().to_string();
    assert!(
        message.contains("npm ci --ignore-scripts --no-fund --no-audit"),
        "{message}"
    );
}

#[test]
fn a_missing_lockfile_counts_as_missing_dependencies() {
    let fx = Fixture::new("no-lock", true);
    fs::rename(
        fx.frontend.join("package-lock.json"),
        fx.frontend.join("package-lock.json.moved"),
    )
    .unwrap();
    let result = build_popover_with(&fx.frontend, &fx.out_dir, |job| {
        write_assets(job.out_dir);
        Ok(())
    });
    assert!(
        matches!(result, Err(BuildError::MissingDependencies { .. })),
        "{result:?}"
    );
    assert!(!fx.staged().exists());
}

#[test]
fn an_absent_node_is_reported_and_nothing_is_staged() {
    let fx = Fixture::new("no-node", true);
    let node = fx.out_dir.join("no-such-node");
    let result = build_popover(&fx.frontend, &fx.out_dir, node.as_os_str());
    assert!(
        matches!(result, Err(BuildError::NodeUnavailable { .. })),
        "{result:?}"
    );
    assert!(!fx.staged().exists());
}

#[test]
fn a_builder_failure_keeps_the_previous_stage_from_counting_as_success() {
    let fx = Fixture::new("builder-fails", true);
    fx.seed_previous_stage();
    let before = Fixture::snapshot(&fx.staged());
    let result = build_popover_with(&fx.frontend, &fx.out_dir, |job| {
        write_assets(job.out_dir);
        Err(BuildError::BuilderFailed {
            status: "exit status: 1".into(),
        })
    });
    assert!(
        matches!(result, Err(BuildError::BuilderFailed { .. })),
        "{result:?}"
    );
    assert_eq!(Fixture::snapshot(&fx.staged()), before);
}

#[test]
fn stale_source_dist_and_previous_stage_do_not_satisfy_a_silent_builder() {
    let fx = Fixture::new("silent-builder", true);
    fx.seed_previous_stage();
    let before = Fixture::snapshot(&fx.staged());
    let result = build_popover_with(&fx.frontend, &fx.out_dir, |_| Ok(()));
    assert!(
        matches!(result, Err(BuildError::InvalidAsset { .. })),
        "{result:?}"
    );
    assert_eq!(Fixture::snapshot(&fx.staged()), before);
}

#[test]
fn every_invocation_builds_into_a_new_empty_directory() {
    let fx = Fixture::new("fresh-dirs", true);
    let mut seen = Vec::new();
    for _ in 0..2 {
        build_popover_with(&fx.frontend, &fx.out_dir, |job| {
            assert!(job.out_dir.starts_with(&fx.out_dir));
            assert_eq!(fs::read_dir(job.out_dir).unwrap().count(), 0);
            seen.push(job.out_dir.to_path_buf());
            write_assets(job.out_dir);
            Ok(())
        })
        .unwrap();
    }
    assert_ne!(seen[0], seen[1]);
}

#[test]
fn an_incomplete_build_is_rejected() {
    let fx = Fixture::new("incomplete", true);
    let result = build_popover_with(&fx.frontend, &fx.out_dir, |job| {
        fs::write(job.out_dir.join("index.html"), "<!doctype html>\n").unwrap();
        fs::write(job.out_dir.join("popover.js"), "js\n").unwrap();
        Ok(())
    });
    assert!(
        matches!(result, Err(BuildError::InvalidAsset { .. })),
        "{result:?}"
    );
    assert!(!fx.staged().exists());
}

#[test]
fn an_empty_asset_is_rejected() {
    let fx = Fixture::new("empty-asset", true);
    let result = build_popover_with(&fx.frontend, &fx.out_dir, |job| {
        write_assets(job.out_dir);
        fs::write(job.out_dir.join("popover.css"), "").unwrap();
        Ok(())
    });
    assert!(
        matches!(result, Err(BuildError::InvalidAsset { .. })),
        "{result:?}"
    );
    assert!(!fx.staged().exists());
}

#[test]
fn a_directory_in_place_of_an_asset_is_rejected() {
    let fx = Fixture::new("dir-asset", true);
    let result = build_popover_with(&fx.frontend, &fx.out_dir, |job| {
        write_assets(job.out_dir);
        fs::rename(job.out_dir.join("popover.js"), job.out_dir.join("moved.js")).unwrap();
        fs::create_dir(job.out_dir.join("popover.js")).unwrap();
        fs::write(job.out_dir.join("popover.js").join("inner"), "x").unwrap();
        Ok(())
    });
    assert!(
        matches!(result, Err(BuildError::InvalidAsset { .. })),
        "{result:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_symlink_to_the_stale_source_dist_is_rejected() {
    let fx = Fixture::new("symlink-asset", true);
    let stale = fx.frontend.join("dist");
    let result = build_popover_with(&fx.frontend, &fx.out_dir, |job| {
        write_assets(job.out_dir);
        fs::rename(job.out_dir.join("popover.js"), job.out_dir.join("moved.js")).unwrap();
        std::os::unix::fs::symlink(stale.join("popover.js"), job.out_dir.join("popover.js"))
            .unwrap();
        Ok(())
    });
    assert!(
        matches!(result, Err(BuildError::InvalidAsset { .. })),
        "{result:?}"
    );
    assert!(!fx.staged().exists());
}

#[test]
fn a_complete_fresh_build_is_staged_and_the_source_dist_is_untouched() {
    let fx = Fixture::new("success", true);
    fx.seed_previous_stage();
    let dist_before = Fixture::snapshot(&fx.frontend.join("dist"));
    let mut fresh = PathBuf::new();
    let staged = build_popover_with(&fx.frontend, &fx.out_dir, |job| {
        assert_eq!(job.vite_cli, vite_cli(&fx.frontend));
        assert_eq!(job.frontend_dir, fx.frontend);
        fresh = job.out_dir.to_path_buf();
        write_assets(job.out_dir);
        Ok(())
    })
    .unwrap();
    // Moved, not copied: the build directory keeps no second bundle.
    assert_eq!(fs::read_dir(&fresh).unwrap().count(), 0);
    assert_eq!(staged, fx.staged());
    for name in REQUIRED_ASSETS {
        assert_eq!(
            fs::read_to_string(staged.join(name)).unwrap(),
            format!("fresh {name}\n")
        );
    }
    assert_eq!(Fixture::snapshot(&fx.frontend.join("dist")), dist_before);
}

/// The real process path: `node <vite.js> build --outDir <fresh>
/// --emptyOutDir=false` run from the
/// frontend dir. `/bin/sh` stands in for Node and the fixture's `vite.js` is a
/// shell script, so nothing freshly written is ever exec'd directly.
#[cfg(unix)]
mod process {
    use super::*;

    fn fake_vite(fx: &Fixture, body: &str) {
        fs::write(vite_cli(&fx.frontend), format!("{body}\n")).unwrap();
    }

    #[test]
    fn a_nonzero_builder_exit_is_a_failure_even_with_assets_written() {
        let fx = Fixture::new("proc-exit", true);
        fake_vite(
            &fx,
            r#"for f in index.html popover.js popover.css; do echo x > "$3/$f"; done; exit 3"#,
        );
        let result = build_popover(&fx.frontend, &fx.out_dir, "/bin/sh".as_ref());
        match result {
            Err(BuildError::BuilderFailed { status }) => assert!(status.contains('3'), "{status}"),
            other => panic!("{other:?}"),
        }
        assert!(!fx.staged().exists());
    }

    #[test]
    fn vite_runs_from_the_frontend_and_writes_only_under_out_dir() {
        let fx = Fixture::new("proc-ok", true);
        let record = fx.out_dir.join("invocation.txt");
        fake_vite(
            &fx,
            &format!(
                r#"case "$0" in */node_modules/vite/bin/vite.js) ;; *) exit 11 ;; esac
[ "$1" = build ] || exit 12
[ "$2" = --outDir ] || exit 13
[ "$4" = --emptyOutDir=false ] || exit 14
printf '%s\n%s\n' "$PWD" "$3" > '{}'
echo 'cargo:rustc-cfg=injected_by_builder'
for f in index.html popover.js popover.css; do echo "fresh $f" > "$3/$f"; done"#,
                record.display()
            ),
        );
        let dist_before = Fixture::snapshot(&fx.frontend.join("dist"));
        let staged = build_popover(&fx.frontend, &fx.out_dir, "/bin/sh".as_ref()).unwrap();

        let recorded = fs::read_to_string(&record).unwrap();
        let mut lines = recorded.lines();
        let cwd = PathBuf::from(lines.next().unwrap());
        let out = PathBuf::from(lines.next().unwrap());
        assert_eq!(
            cwd.canonicalize().unwrap(),
            fx.frontend.canonicalize().unwrap()
        );
        assert!(out.starts_with(&fx.out_dir), "{}", out.display());
        assert_ne!(out, fx.staged());
        assert_eq!(
            fs::read_to_string(staged.join("popover.css")).unwrap(),
            "fresh popover.css\n"
        );
        assert_eq!(Fixture::snapshot(&fx.frontend.join("dist")), dist_before);
    }
}
