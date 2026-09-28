//! Private, bounded account-switch transport. This is never a terminal UI.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::core::accounts::{self, Effect, Failure, Provider, SwitchRequest, VERSION};

const WORKER_ARG: &str = "--internal-account-worker-v1";
const MAX_REQUEST_BYTES: usize = 64 * 1024;
const MAX_RESPONSE_BYTES: usize = 16 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    version: u8,
    result: Result<Effect, Failure>,
}

fn read_bounded(reader: impl Read, limit: usize) -> Result<Vec<u8>, ()> {
    let mut bytes = Vec::new();
    reader
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.len() > limit {
        return Err(());
    }
    Ok(bytes)
}

fn parse_request(bytes: &[u8]) -> Result<SwitchRequest, Failure> {
    if bytes.len() > MAX_REQUEST_BYTES
        || bytes.iter().find(|b| !b.is_ascii_whitespace()) != Some(&b'{')
    {
        return Err(Failure::InvalidRequest);
    }
    let request: SwitchRequest =
        serde_json::from_slice(bytes).map_err(|_| Failure::InvalidRequest)?;
    request.validate()?;
    Ok(request)
}

fn serve(
    reader: impl Read,
    mut writer: impl Write,
    execute: impl FnOnce(&SwitchRequest) -> Result<Effect, Failure>,
) -> i32 {
    let result = read_bounded(reader, MAX_REQUEST_BYTES)
        .map_err(|_| Failure::InvalidRequest)
        .and_then(|bytes| parse_request(&bytes))
        .and_then(|request| execute(&request));
    let code = i32::from(result.is_err());
    let response = Response {
        version: VERSION,
        result,
    };
    // This schema contains only enums, so its encoded size is constant-bounded.
    match serde_json::to_vec(&response) {
        Ok(bytes) if bytes.len() <= MAX_RESPONSE_BYTES => {
            if writer
                .write_all(&bytes)
                .and_then(|_| writer.flush())
                .is_err()
            {
                return 2;
            }
        }
        _ => return 2,
    }
    code
}

fn execute(request: &SwitchRequest) -> Result<Effect, Failure> {
    // A broken config must fail closed, never choose conventional accounts as
    // a fallback. None of the raw parser diagnostics leave this process.
    let config = crate::config::Config::load().map_err(|_| Failure::InvalidConfiguration)?;
    let claude_marker = crate::anthropic::cli_account::home_claude_json()
        .map_err(|_| Failure::StorageUnavailable)?;
    let codex_default =
        crate::openai::creds::default_path().map_err(|_| Failure::StorageUnavailable)?;
    let effect = accounts::switch_at(
        request,
        &config,
        &claude_marker,
        &codex_default,
        &crate::anthropic::cli_account::KeychainStore,
    )?;
    if effect.invalidates_default_cache()
        && let Ok(cache) = crate::cache::Cache::for_vendor(request.provider.cache_vendor())
    {
        cache.forget_after_fetch();
    }
    Ok(effect)
}

/// Called before any GUI setup in the tray executable. Only the exact
/// internal mode is accepted; extra arguments cannot select a path or command.
#[doc(hidden)]
pub fn run_if_requested() -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(WORKER_ARG)) {
        return None;
    }
    if args.next().is_some() {
        return Some(2);
    }
    Some(serve(
        std::io::stdin().lock(),
        std::io::stdout().lock(),
        execute,
    ))
}

fn parse_response(bytes: &[u8], success: bool) -> Result<Effect, Failure> {
    if bytes.len() > MAX_RESPONSE_BYTES
        || bytes.iter().find(|b| !b.is_ascii_whitespace()) != Some(&b'{')
    {
        return Err(Failure::InvalidResponse);
    }
    let response: Response = serde_json::from_slice(bytes).map_err(|_| Failure::InvalidResponse)?;
    if response.version != VERSION || response.result.is_ok() != success {
        return Err(Failure::InvalidResponse);
    }
    response.result
}

/// The process stays alive until its credential transaction finishes. No
/// timeout kills it between mutation and rollback. Stderr is discarded, and
/// stdout is bounded before waiting, so raw subprocess diagnostics cannot leak.
pub(crate) fn switch_with(tray: &Path, vendor: &str, label: &str) -> Result<Effect, Failure> {
    let provider = Provider::from_vendor(vendor).ok_or(Failure::InvalidRequest)?;
    let request = SwitchRequest::new(provider, label.to_owned())?;
    let bytes = serde_json::to_vec(&request).map_err(|_| Failure::InvalidRequest)?;
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(Failure::InvalidRequest);
    }
    let mut child = Command::new(tray)
        .arg(WORKER_ARG)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Failure::WorkerUnavailable)?;
    let written = child
        .stdin
        .take()
        .is_some_and(|mut stdin| stdin.write_all(&bytes).is_ok());
    // read_bounded owns and drops the pipe, including on excess output. Waiting
    // with an unread full stdout pipe would deadlock the parent and the worker.
    let output = child
        .stdout
        .take()
        .ok_or(())
        .and_then(|stdout| read_bounded(stdout, MAX_RESPONSE_BYTES));
    let status = child.wait().map_err(|_| Failure::InvalidResponse)?;
    if !written {
        return Err(Failure::InvalidResponse);
    }
    let output = output.map_err(|_| Failure::InvalidResponse)?;
    parse_response(&output, status.success())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn round_trip(input: &[u8], effect: Result<Effect, Failure>) -> (i32, Vec<u8>, bool) {
        let called = Cell::new(false);
        let mut output = Vec::new();
        let status = serve(input, &mut output, |_| {
            called.set(true);
            effect
        });
        (status, output, called.get())
    }

    #[test]
    fn malformed_requests_never_reach_account_services() {
        for input in [
            r#"{}"#,
            r#"[1,"codex","work"]"#,
            r#"{"version":2,"provider":"codex","label":"work"}"#,
            r#"{"version":1,"provider":"copilot","label":"work"}"#,
            r#"{"version":1,"provider":"codex","label":"../work"}"#,
            r#"{"version":1,"provider":"codex","label":"work","force":true}"#,
            r#"{"version":1,"provider":"codex","label":"work","path":"/tmp/custom"}"#,
            r#"{"version":1,"provider":"codex","label":"work","label":"other"}"#,
            r#"{"version":1,"provider":"codex","label":"work"} {}"#,
        ] {
            let (status, output, called) = round_trip(input.as_bytes(), Ok(Effect::Switched));
            assert!(!called, "invalid request reached credentials");
            assert_eq!(status, 1);
            assert_eq!(parse_response(&output, false), Err(Failure::InvalidRequest));
        }
        let (_, _, called) = round_trip(&vec![b' '; MAX_REQUEST_BYTES + 1], Ok(Effect::Switched));
        assert!(!called);
    }

    #[test]
    fn structured_success_and_failure_round_trip_without_input_echo() {
        let request = SwitchRequest::new(Provider::Codex, "private-account-label".into()).unwrap();
        let input = serde_json::to_vec(&request).unwrap();
        for effect in [
            Ok(Effect::Switched),
            Ok(Effect::AlreadyActive),
            Err(Failure::InvalidConfiguration),
            Err(Failure::SwitchFailed),
        ] {
            let (status, bytes, called) = round_trip(&input, effect);
            assert!(called);
            assert_eq!(parse_response(&bytes, status == 0), effect);
            assert!(
                !String::from_utf8(bytes)
                    .unwrap()
                    .contains("private-account-label")
            );
        }
    }

    #[test]
    fn invalid_or_inconsistent_worker_responses_are_rejected() {
        for input in [
            r#"{"version":2,"result":{"Ok":"switched"}}"#,
            r#"{"version":1,"result":{"Ok":"forced"}}"#,
            r#"{"version":1,"result":{"Ok":"switched"},"detail":"private"}"#,
            r#"{"version":1,"result":{"Err":"switch_failed"}}"#,
            "private stderr text",
        ] {
            assert_eq!(
                parse_response(input.as_bytes(), true),
                Err(Failure::InvalidResponse)
            );
        }
        assert_eq!(
            parse_response(br#"{"version":1,"result":{"Ok":"switched"}}"#, false),
            Err(Failure::InvalidResponse)
        );
        assert!(read_bounded(&vec![0; MAX_RESPONSE_BYTES + 1][..], MAX_RESPONSE_BYTES).is_err());
        assert_eq!(read_bounded(&b"abcd"[..], 4).unwrap(), b"abcd");
    }

    #[test]
    fn invalid_provider_and_label_do_not_launch_a_process() {
        let nonexistent = Path::new("/not-a-real-executable");
        assert_eq!(
            switch_with(nonexistent, "claude_desktop", "work"),
            Err(Failure::InvalidRequest)
        );
        assert_eq!(
            switch_with(nonexistent, "openai", "../work"),
            Err(Failure::InvalidRequest)
        );
        assert_eq!(
            switch_with(nonexistent, "openai", "work"),
            Err(Failure::WorkerUnavailable)
        );
    }
}
