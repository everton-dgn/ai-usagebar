//! Exercise the actual private worker without GUI setup or real accounts.

#![cfg(target_os = "macos")]

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};

const WORKER_ARG: &str = "--internal-account-worker-v1";

fn run(home: &Path, args: &[&str], input: &[u8]) -> Output {
    // These subprocess tests exercise Codex only, with every path in the
    // fixture. PATH does not isolate Keychain utilities invoked by absolute
    // path; Claude tests must inject a fake CredentialStore in-process.
    let mut child = Command::new(env!("CARGO_BIN_EXE_ai-usagebar-tray"))
        .args(args)
        .env_clear()
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("PATH", home.join("no-executables"))
        .current_dir(home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn config(home: &Path, contents: &str) -> std::path::PathBuf {
    let path = home.join("Library/Application Support/ai-usagebar/config.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, contents).unwrap();
    path
}

fn request() -> Vec<u8> {
    serde_json::to_vec(&json!({"version":1,"provider":"codex","label":"work"})).unwrap()
}

fn assert_result(output: Output, success: bool, result: Value) {
    assert_eq!(output.status.success(), success);
    assert!(output.stderr.is_empty(), "worker must not emit diagnostics");
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        json!({"version":1,"result":result})
    );
}

#[test]
fn malformed_input_and_extra_arguments_exit_before_gui_or_account_work() {
    let home = tempfile::tempdir().unwrap();
    assert_result(
        run(home.path(), &[WORKER_ARG], b"not-json"),
        false,
        json!({"Err":"invalid_request"}),
    );
    let extra = run(home.path(), &[WORKER_ARG, "--force"], b"");
    assert_eq!(extra.status.code(), Some(2));
    assert!(extra.stdout.is_empty());
    assert!(extra.stderr.is_empty());
    assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 0);
}

#[test]
fn invalid_configuration_fails_closed_without_echoing_its_contents() {
    let home = tempfile::tempdir().unwrap();
    let malformed = "provider = [ fixture-private-value";
    let path = config(home.path(), malformed);
    assert_result(
        run(home.path(), &[WORKER_ARG], &request()),
        false,
        json!({"Err":"invalid_configuration"}),
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), malformed);
    assert!(!home.path().join(".codex").exists());
}

#[test]
fn missing_configuration_does_not_discover_or_modify_conventional_accounts() {
    let home = tempfile::tempdir().unwrap();
    assert_result(
        run(home.path(), &[WORKER_ARG], &request()),
        false,
        json!({"Err":"unknown_account"}),
    );
    assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 0);
}

#[test]
fn codex_transaction_completes_in_the_private_worker_and_is_idempotent() {
    let home = tempfile::tempdir().unwrap();
    let named = home.path().join("work/auth.json");
    std::fs::create_dir_all(named.parent().unwrap()).unwrap();
    let auth = json!({"tokens":{"account_id":"fixture-account","access_token":"fixture-access","refresh_token":"fixture-refresh","id_token":"x.y.z"}}).to_string();
    std::fs::write(&named, &auth).unwrap();
    let cfg = format!(
        "[[openai.accounts]]\nlabel = \"work\"\ncodex_auth_path = {}\n",
        toml::Value::String(named.to_str().unwrap().to_owned())
    );
    let path = config(home.path(), &cfg);

    for effect in ["switched", "already_active"] {
        assert_result(
            run(home.path(), &[WORKER_ARG], &request()),
            true,
            json!({"Ok":effect}),
        );
        assert_eq!(
            std::fs::read_to_string(home.path().join(".codex/auth.json")).unwrap(),
            auth
        );
        assert!(!named.exists());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), cfg);
    }
}
