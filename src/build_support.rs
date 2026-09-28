//! Fail-closed build of the tray WebView frontend, shared by `build.rs` and
//! its hermetic tests (`tests/build_support.rs`). Not part of the library.
//!
//! Std-only on purpose: a build script sees only `[build-dependencies]`.
//!
//! The contract `build.rs` relies on:
//! - dependencies are never installed here; a missing lockfile or local Vite
//!   is an error naming the explicit install command;
//! - Vite runs through Node directly (`node node_modules/vite/bin/vite.js`),
//!   never through an npm/pnpm wrapper that could install on its own;
//! - every invocation builds into a directory it just created empty under
//!   `OUT_DIR`, so the source `dist/` and any previous build can never count;
//! - the three assets must be non-empty regular files from that directory
//!   before any is moved into `OUT_DIR/popover`. Moving (not copying) leaves
//!   only an empty `popover-build-N` behind; nothing is ever deleted, so those
//!   empty directories accumulate until `cargo clean`.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The files the hosts `include_str!` from `OUT_DIR/popover`.
pub const REQUIRED_ASSETS: [&str; 3] = ["index.html", "popover.js", "popover.css"];

const LOCKFILE: &str = "package-lock.json";
const INSTALL_HINT: &str = "npm ci --ignore-scripts --no-fund --no-audit";
const STAGED_DIR: &str = "popover";
const BUILD_DIR_PREFIX: &str = "popover-build-";
const MAX_BUILD_DIRS: u32 = 10_000;

/// One builder run: produce the assets inside `out_dir`, which is new and empty.
pub struct BuildJob<'a> {
    pub frontend_dir: &'a Path,
    pub vite_cli: &'a Path,
    pub out_dir: &'a Path,
}

#[derive(Debug)]
pub enum BuildError {
    MissingDependencies {
        frontend_dir: PathBuf,
        missing: PathBuf,
    },
    NodeUnavailable {
        program: OsString,
        source: io::Error,
    },
    BuilderFailed {
        status: String,
    },
    InvalidAsset {
        path: PathBuf,
        reason: &'static str,
    },
    Io {
        context: String,
        source: io::Error,
    },
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingDependencies {
                frontend_dir,
                missing,
            } => write!(
                f,
                "frontend dependencies are not installed ({} is missing); \
                 run `{INSTALL_HINT}` in {} first. The build never installs them.",
                missing.display(),
                frontend_dir.display()
            ),
            Self::NodeUnavailable { program, source } => write!(
                f,
                "Node.js is required to build the tray frontend but {} could not be started: {source}",
                Path::new(program).display()
            ),
            Self::BuilderFailed { status } => write!(f, "Vite build failed ({status})"),
            Self::InvalidAsset { path, reason } => {
                write!(f, "Vite build did not produce {}: {reason}", path.display())
            }
            Self::Io { context, source } => write!(f, "{context}: {source}"),
        }
    }
}

/// Build with the real Vite through `node` and stage the result.
pub fn build_popover(
    frontend_dir: &Path,
    out_dir: &Path,
    node: &OsStr,
) -> Result<PathBuf, BuildError> {
    build_popover_with(frontend_dir, out_dir, |job| run_vite(node, job))
}

/// Check dependencies, run `builder` into a fresh directory, validate, stage.
/// Nothing is staged unless every step succeeds.
pub fn build_popover_with(
    frontend_dir: &Path,
    out_dir: &Path,
    builder: impl FnOnce(&BuildJob<'_>) -> Result<(), BuildError>,
) -> Result<PathBuf, BuildError> {
    let vite_cli = frontend_dir
        .join("node_modules")
        .join("vite")
        .join("bin")
        .join("vite.js");
    for required in [frontend_dir.join(LOCKFILE), vite_cli.clone()] {
        if !is_regular_file(&required) {
            return Err(BuildError::MissingDependencies {
                frontend_dir: frontend_dir.to_path_buf(),
                missing: required,
            });
        }
    }

    let fresh = create_fresh_dir(out_dir)?;
    builder(&BuildJob {
        frontend_dir,
        vite_cli: &vite_cli,
        out_dir: &fresh,
    })?;
    for name in REQUIRED_ASSETS {
        validate_asset(&fresh.join(name))?;
    }

    let staged = out_dir.join(STAGED_DIR);
    fs::create_dir_all(&staged).map_err(|source| BuildError::Io {
        context: format!("create {}", staged.display()),
        source,
    })?;
    // Same filesystem (both under OUT_DIR); `rename` replaces an existing
    // file on Unix and on Windows (MOVEFILE_REPLACE_EXISTING).
    for name in REQUIRED_ASSETS {
        let (from, to) = (fresh.join(name), staged.join(name));
        fs::rename(&from, &to).map_err(|source| BuildError::Io {
            context: format!("move {} to {}", from.display(), to.display()),
            source,
        })?;
    }
    Ok(staged)
}

fn run_vite(node: &OsStr, job: &BuildJob<'_>) -> Result<(), BuildError> {
    let status = Command::new(node)
        .arg(job.vite_cli)
        .arg("build")
        .arg("--outDir")
        .arg(job.out_dir)
        // The directory is always new; keep Vite from emptying anything. The
        // `=` form matters: `--emptyOutDir false` would parse `false` as root.
        .arg("--emptyOutDir=false")
        .current_dir(job.frontend_dir)
        .stdin(Stdio::null())
        // Cargo parses a build script's stdout for `cargo:` directives; the
        // builder's output is diagnostics only.
        .stdout(Stdio::from(io::stderr()))
        .status()
        .map_err(|source| {
            if source.kind() == io::ErrorKind::NotFound {
                BuildError::NodeUnavailable {
                    program: node.to_os_string(),
                    source,
                }
            } else {
                BuildError::Io {
                    context: format!("start {}", Path::new(node).display()),
                    source,
                }
            }
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(BuildError::BuilderFailed {
            status: status.to_string(),
        })
    }
}

/// `create_dir`, not `create_dir_all`: success proves the directory did not
/// exist, so whatever is in it afterwards came from this invocation.
fn create_fresh_dir(out_dir: &Path) -> Result<PathBuf, BuildError> {
    for n in 0..MAX_BUILD_DIRS {
        let candidate = out_dir.join(format!("{BUILD_DIR_PREFIX}{n}"));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(BuildError::Io {
                    context: format!("create {}", candidate.display()),
                    source,
                });
            }
        }
    }
    Err(BuildError::Io {
        context: format!("no free {BUILD_DIR_PREFIX}N under {}", out_dir.display()),
        source: io::Error::from(io::ErrorKind::AlreadyExists),
    })
}

fn validate_asset(path: &Path) -> Result<(), BuildError> {
    let invalid = |reason| BuildError::InvalidAsset {
        path: path.to_path_buf(),
        reason,
    };
    // `symlink_metadata`: a link back to a stale `dist/` is not this build's output.
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Err(invalid("missing")),
        Err(source) => {
            return Err(BuildError::Io {
                context: format!("inspect {}", path.display()),
                source,
            });
        }
    };
    if !meta.file_type().is_file() {
        return Err(invalid("not a regular file"));
    }
    if meta.len() == 0 {
        return Err(invalid("empty"));
    }
    Ok(())
}

fn is_regular_file(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.is_file())
}
