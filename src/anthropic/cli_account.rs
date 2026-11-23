//! Which Claude account the `claude` **CLI** is signed into, and moving that
//! login between managed accounts.
//!
//! This is a separate identity from the Claude Desktop app's (see
//! [`crate::claude_desktop`]) — the two drift apart constantly, which is most
//! of why they are both worth reporting.
//!
//! The CLI keeps exactly **one** default login: `~/.claude/.credentials.json`
//! on Linux, the login-Keychain item `Claude Code-credentials` on macOS. A
//! named account instead lives under its own `CLAUDE_CONFIG_DIR`, in the
//! per-directory Keychain item [`crate::anthropic::keychain`] resolves. Making
//! a named account "the one plain `claude` uses" therefore means moving its
//! credential into that single default slot.
//!
//! Copying would leave both slots holding the same *rotating* refresh token,
//! and whichever client refreshed first would invalidate the other — the
//! failure [`crate::anthropic::creds`] documents. The switch therefore moves
//! the credential: it captures the outgoing default credential back into its
//! named slot first, installs the target in the default slot, and removes the
//! target's named copy. In addition,
//! [`crate::config::AnthropicConfig::account_target_with`] routes reads for
//! whichever label is *currently* active to the default slot, so only one copy
//! is ever live.
//!
//! Who a credential belongs to is read from the credential: Claude Code keeps
//! the account uuid beside the tokens. The `oauthAccount` marker in
//! `~/.claude.json` is written here but never trusted, because every running
//! Claude Code session rewrites that file. Which account a *label* is comes
//! from this app's own record ([`IDENTITY_FILE`]); see `docs/accounts.md`.
//!
//! Everything except [`KeychainStore`] is platform-agnostic and unit-tested
//! against a fake store, so the logic stays under CI's linter on Linux.

use std::path::Path;
use std::time::Duration;

use serde_json::{Map, Value};

use crate::config::AnthropicAccount;
use crate::error::{AccountFailure, AppError, Result};

/// The `claude` CLI's per-config-dir state file. Holds a plaintext
/// `oauthAccount` identity marker (uuid, email, org) — never a token.
const CLAUDE_JSON: &str = ".claude.json";

/// The lock every mutation of the CLI's credential slots holds: a switch, an
/// adoption, and a fetch from reading a credential until its refreshed tokens
/// are written back.
pub fn switch_lock_path(home_claude_json: &Path) -> std::path::PathBuf {
    home_claude_json
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(".ai-usagebar-account-switch.lock")
}

/// Read/write access to the credential slots. Abstracted so the switch logic
/// can be tested without a macOS Keychain.
pub trait CredentialStore {
    fn read_default(&self) -> Result<Option<String>>;
    fn write_default(&self, blob: &str) -> Result<()>;
    fn delete_default(&self) -> Result<()>;
    fn read_named(&self, config_dir: &Path) -> Result<Option<String>>;
    fn write_named(&self, config_dir: &Path, blob: &str) -> Result<()>;
    fn delete_named(&self, config_dir: &Path) -> Result<()>;
}

/// The real store. The `#[cfg]` pairs are confined here so no other logic in
/// this module is invisible to a Linux build — the same shape
/// [`crate::anthropic::creds::resolve`] uses.
pub struct KeychainStore;

/// Path to the home `.claude.json` that records the live CLI identity.
pub fn home_claude_json() -> Result<std::path::PathBuf> {
    Ok(crate::cache::home_dir()?.join(CLAUDE_JSON))
}

/// The identity marker inside an account's own `CLAUDE_CONFIG_DIR`.
pub fn marker_path(config_dir: &Path) -> std::path::PathBuf {
    config_dir.join(CLAUDE_JSON)
}

/// The account UUID recorded in a `.claude.json`, if any.
pub fn account_uuid_in(claude_json: &Path) -> Option<String> {
    oauth_account_in(claude_json)?
        .get("accountUuid")?
        .as_str()
        .filter(|uuid| !uuid.is_empty())
        .map(str::to_string)
}

/// The e-mail recorded alongside it, for display only.
pub fn account_email_in(claude_json: &Path) -> Option<String> {
    oauth_account_in(claude_json)?
        .get("emailAddress")?
        .as_str()
        .filter(|email| !email.is_empty())
        .map(str::to_string)
}

/// This app's record of which account a label is, beside the account's
/// `.claude.json`.
const IDENTITY_FILE: &str = ".ai-usagebar-identity.json";

/// Where the identity record of the account in `config_dir` lives.
pub fn identity_path(config_dir: &Path) -> std::path::PathBuf {
    config_dir.join(IDENTITY_FILE)
}

/// The account a stored credential was issued to. `None` for a blob that does
/// not say: such a credential names nobody, and is never assumed to be the
/// slot's.
pub fn blob_account_uuid(blob: &str) -> Option<String> {
    let document: Value = serde_json::from_str(blob).ok()?;
    document
        .get("claudeAiOauth")?
        .get("accountUuid")?
        .as_str()
        .filter(|uuid| !uuid.is_empty())
        .map(str::to_string)
}

/// The recorded `oauthAccount` of the account in `config_dir`.
fn recorded_identity(config_dir: &Path) -> Option<Value> {
    let bytes = std::fs::read(identity_path(config_dir)).ok()?;
    let record: Value = serde_json::from_slice(&bytes).ok()?;
    let oauth_account = record.get("oauth_account")?;
    uuid_of(oauth_account)
        .is_some()
        .then(|| oauth_account.clone())
}

fn uuid_of(oauth_account: &Value) -> Option<&str> {
    oauth_account
        .get("accountUuid")?
        .as_str()
        .filter(|uuid| !uuid.is_empty())
}

/// The `oauthAccount` that describes `account`: this app's record, or, until
/// one is written, the marker Claude Code left in the account's directory.
fn account_marker(account: &AnthropicAccount) -> Option<Value> {
    let dir = account.config_dir();
    recorded_identity(&dir)
        .or_else(|| oauth_account_in(&marker_path(&dir)).filter(|marker| uuid_of(marker).is_some()))
}

/// Which account `account` is.
pub fn account_identity(account: &AnthropicAccount) -> Option<String> {
    uuid_of(&account_marker(account)?).map(str::to_string)
}

/// The e-mail of the account `account` is, for display only.
pub fn account_email(account: &AnthropicAccount) -> Option<String> {
    account_marker(account)?
        .get("emailAddress")?
        .as_str()
        .filter(|email| !email.is_empty())
        .map(str::to_string)
}

/// Record the identity of every account that has none yet, from the marker in
/// its directory.
///
/// WHY: a later sign-in in that directory rewrites the marker to whichever
/// account the browser authorized. The record keeps the label on the account
/// it was set up for, so a credential issued to anyone else is recognised as
/// foreign. Best-effort: an unrecorded account keeps resolving through its
/// marker.
pub fn record_identities(accounts: &[AnthropicAccount]) {
    for account in accounts {
        let dir = account.config_dir();
        if recorded_identity(&dir).is_some() {
            continue;
        }
        let Some(marker) = account_marker(account) else {
            continue;
        };
        // Two labels on one account: recording either would settle an
        // ambiguity nobody has resolved.
        if owner_of(uuid_of(&marker).unwrap_or_default(), accounts).is_none() {
            continue;
        }
        if let Ok(bytes) =
            serde_json::to_vec_pretty(&serde_json::json!({ "oauth_account": marker }))
        {
            let _ = crate::cache::atomic_write(&identity_path(&dir), &bytes);
        }
    }
}

/// The managed account `uuid` belongs to. `None` when no label or more than
/// one claims it, since either answer could be wrong.
pub fn owner_of<'a>(uuid: &str, accounts: &'a [AnthropicAccount]) -> Option<&'a AnthropicAccount> {
    let mut owners = accounts
        .iter()
        .filter(|account| account_identity(account).as_deref() == Some(uuid));
    let owner = owners.next()?;
    owners.next().is_none().then_some(owner)
}

/// Which managed label the login in the default slot belongs to, given that
/// slot's credential.
///
/// `None` when the CLI is signed into something we do not manage — a plain
/// `claude` login done by hand, say — or into a credential that does not say
/// whose it is. Callers surface that as "unknown" rather than guessing: it is
/// exactly the case where the named copies may have gone stale behind our back.
pub fn resolve_active_label(
    default_blob: Option<&str>,
    accounts: &[AnthropicAccount],
) -> Option<String> {
    let live = blob_account_uuid(default_blob?)?;
    owner_of(&live, accounts).map(|account| account.label.clone())
}

/// [`resolve_active_label`] against the real default slot. An unreadable
/// Keychain is "unknown", like a login nobody manages.
pub fn active_label(accounts: &[AnthropicAccount]) -> Option<String> {
    let blob = KeychainStore.read_default().ok().flatten();
    resolve_active_label(blob.as_deref(), accounts)
}

/// Set (or clear) the `oauthAccount` marker in a `.claude.json`, preserving
/// every other key. That document is tens of kilobytes of unrelated CLI state,
/// so it is merged into, never replaced.
pub fn merge_oauth_account(existing: &[u8], oauth_account: Option<&Value>) -> Result<Vec<u8>> {
    let mut document: Value = if existing.iter().all(u8::is_ascii_whitespace) {
        Value::Object(Map::new())
    } else {
        serde_json::from_slice(existing)?
    };
    let object = document
        .as_object_mut()
        .ok_or_else(|| AppError::Other(format!("{CLAUDE_JSON} is not a JSON object")))?;
    match oauth_account {
        Some(account) => {
            object.insert("oauthAccount".into(), account.clone());
        }
        // Better an absent marker than one that names the previous account.
        None => {
            object.remove("oauthAccount");
        }
    }
    Ok(serde_json::to_vec(&document)?)
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CliSwitchOpts {
    /// Overwrite the default slot even though the live login belongs to no
    /// managed account. That login cannot be captured anywhere first, so this
    /// genuinely destroys it.
    pub force: bool,
    /// Validate everything and report, without writing.
    pub dry_run: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CliSwitchOutcome {
    AlreadyActive,
    /// The account was already active, and a redundant named Keychain copy was
    /// removed so the rotating token again has one live lineage.
    RemovedDuplicate,
    /// Dry-run counterpart of [`Self::RemovedDuplicate`].
    WouldRemoveDuplicate,
    /// The identity marker named this account but the default credential slot
    /// was empty; its named credential was moved into the default slot.
    RepairedActive,
    /// `outgoing` is the label whose credential was captured back first, when
    /// there was one.
    Switched {
        outgoing: Option<String>,
    },
    /// `dry_run` was set; nothing was written.
    WouldSwitch {
        outgoing: Option<String>,
    },
}

/// Make `label` the account plain `claude` uses.
///
/// Ordering is deliberate. The outgoing credential is captured back **before**
/// anything is overwritten — that is the only irreversible step, and a failure
/// there aborts. The default slot is then written **before** the identity
/// marker, because the credential is the source of truth: a marker claiming an
/// account whose token is not there is worse than the reverse.
pub fn switch_cli_account(
    home_claude_json: &Path,
    accounts: &[AnthropicAccount],
    label: &str,
    opts: CliSwitchOpts,
    store: &dyn CredentialStore,
) -> Result<CliSwitchOutcome> {
    let lock_path = switch_lock_path(home_claude_json);
    let _lock = crate::cache::acquire_lock(&lock_path, Duration::from_secs(2))
        .map_err(|error| error.for_account(AccountFailure::LockUnavailable))?;

    let target = accounts
        .iter()
        .find(|account| account.label == label)
        .ok_or_else(|| {
            let known: Vec<&str> = accounts.iter().map(|a| a.label.as_str()).collect();
            AppError::Credentials(format!(
                "no Claude CLI account {label:?} in [[anthropic.accounts]] or accounts_dir; \
                 known: {known:?}"
            ))
        })?;

    let storage = |error: AppError| error.for_account(AccountFailure::StorageUnavailable);

    // Who the target is must be known, and known to be one label only, before
    // any credential can be matched against it.
    let oauth_account = account_marker(target)
        .filter(|marker| {
            uuid_of(marker)
                .and_then(|uuid| owner_of(uuid, accounts))
                .is_some()
        })
        .ok_or_else(|| {
            AppError::Credentials(format!(
                "the identity marker for {label:?} is missing, invalid or shared with another \
                 account; that account needs a new sign-in before switching"
            ))
            .for_account(AccountFailure::InvalidState)
        })?;
    let target_uuid = uuid_of(&oauth_account).unwrap_or_default().to_string();

    // Stray credentials go home first, so the checks below see every account's
    // own slot. This is the one write a refused switch can leave behind.
    if !opts.dry_run {
        let live = store.read_default().map_err(storage)?;
        let live_owner = resolve_active_label(live.as_deref(), accounts);
        rehome_misfiled(accounts, store, live_owner.as_deref())?;
    }

    // Read every piece needed for both the move and its rollback before the
    // first write of the switch itself, and after the reads above: Claude Code
    // may rotate the live token at any moment, and an older copy saved here
    // would be a dead one. An empty default slot is safe to populate without
    // --force; an unrecognised *existing* login is the destructive case.
    let original_default = store.read_default().map_err(storage)?;
    // The login being replaced is whoever its credential says it is. The home
    // marker is not asked: it named one account while this slot held another,
    // and the outgoing login was saved under the wrong label.
    let default_uuid = original_default.as_deref().and_then(blob_account_uuid);
    let outgoing_account = default_uuid
        .as_deref()
        .and_then(|uuid| owner_of(uuid, accounts));
    let active = outgoing_account.map(|account| account.label.clone());

    if active.as_deref() == Some(label) {
        let Some(named) = store.read_named(&target.config_dir()).map_err(storage)? else {
            return Ok(CliSwitchOutcome::AlreadyActive);
        };
        // Only a proven copy of this same login is redundant. A credential
        // issued to someone else, or one that does not say, stays where it is;
        // the account's card reports it.
        if blob_account_uuid(&named).as_deref() != Some(target_uuid.as_str()) {
            return Ok(CliSwitchOutcome::AlreadyActive);
        }
        if opts.dry_run {
            return Ok(CliSwitchOutcome::WouldRemoveDuplicate);
        }
        store.delete_named(&target.config_dir()).map_err(storage)?;
        return Ok(CliSwitchOutcome::RemovedDuplicate);
    }
    if active.is_none() && original_default.is_some() && !opts.force {
        return Err(match default_uuid {
            Some(_) => AppError::Credentials(format!(
                "Claude Code is signed into an account that is not managed here, so \
                 switching to {label:?} would overwrite a login that cannot be saved first."
            ))
            .for_account(AccountFailure::UnmanagedLogin),
            None => AppError::Credentials(format!(
                "the live Claude Code login does not say which account it belongs to, so \
                 switching to {label:?} could not save it under the right account first."
            ))
            .for_account(AccountFailure::UnidentifiedLogin),
        });
    }

    // Fail before the switch writes anything if the target has never been
    // signed in.
    let target_blob = store
        .read_named(&target.config_dir())
        .map_err(storage)?
        .ok_or_else(|| {
            AppError::Credentials(format!(
                "no stored credential for {label:?}; that account has never been signed in \
             here"
            ))
            .for_account(AccountFailure::MissingLogin)
        })?;
    // Installing it makes it the live login, so it must be proven the
    // target's: a blob that does not say would leave the default slot naming
    // nobody, and every later switch refused.
    match blob_account_uuid(&target_blob) {
        Some(uuid) if uuid == target_uuid => {}
        Some(_) => {
            return Err(AppError::Credentials(format!(
                "the credential saved for {label:?} was issued to another account; that \
                 account needs a new sign-in before switching"
            ))
            .for_account(AccountFailure::WrongAccount));
        }
        None => {
            return Err(AppError::Credentials(format!(
                "the credential saved for {label:?} does not say which account it belongs \
                 to; that account needs a new sign-in before switching"
            ))
            .for_account(AccountFailure::UnidentifiedLogin));
        }
    }
    // Only names the outcome: with an empty default slot there is no login to
    // misattribute.
    let marker_named_target = original_default.is_none()
        && account_uuid_in(home_claude_json).as_deref() == Some(target_uuid.as_str());
    // Checked now so an unreadable home file fails before any write; the
    // content that is written is read again right before the write.
    merge_oauth_account(
        read_optional(home_claude_json)?
            .as_deref()
            .unwrap_or_default(),
        Some(&oauth_account),
    )?;

    let original_outgoing_named = match outgoing_account {
        Some(account) => store.read_named(&account.config_dir()).map_err(storage)?,
        None => None,
    };

    if opts.dry_run {
        return Ok(CliSwitchOutcome::WouldSwitch { outgoing: active });
    }

    let credentials = RollbackState {
        target,
        target_blob: &target_blob,
        outgoing: outgoing_account,
        original_outgoing_named: original_outgoing_named.as_deref(),
        original_default: original_default.as_deref(),
        marker: None,
    };

    if let (Some(account), Some(blob)) = (outgoing_account, original_default.as_deref())
        && let Err(error) = store.write_named(&account.config_dir(), blob)
    {
        return Err(with_rollback(error, rollback_switch(store, &credentials)));
    }
    if let Err(error) = store.write_default(&target_blob) {
        return Err(with_rollback(error, rollback_switch(store, &credentials)));
    }

    // WHY: every running Claude Code session rewrites this file. Reading it
    // only now keeps what they wrote during the Keychain work above.
    let original_marker = match read_optional(home_claude_json) {
        Ok(marker) => marker,
        Err(error) => return Err(with_rollback(error, rollback_switch(store, &credentials))),
    };
    let merged_marker = match merge_oauth_account(
        original_marker.as_deref().unwrap_or_default(),
        Some(&oauth_account),
    ) {
        Ok(marker) => marker,
        Err(error) => return Err(with_rollback(error, rollback_switch(store, &credentials))),
    };
    let rollback = RollbackState {
        marker: Some((home_claude_json, original_marker.as_deref())),
        ..credentials
    };
    if let Err(error) = crate::cache::atomic_write(home_claude_json, &merged_marker) {
        return Err(with_rollback(error, rollback_switch(store, &rollback)));
    }
    if let Err(error) = store.delete_named(&target.config_dir()) {
        return Err(with_rollback(error, rollback_switch(store, &rollback)));
    }

    if marker_named_target {
        Ok(CliSwitchOutcome::RepairedActive)
    } else {
        Ok(CliSwitchOutcome::Switched { outgoing: active })
    }
}

/// Move each saved credential that sits in another account's slot to the slot
/// of the account it was issued to.
///
/// The owner must have no login anywhere, saved or live (`live_owner`):
/// a second copy would put one rotating refresh token in two places. Anything
/// else stays put and the holder's card reports it.
fn rehome_misfiled(
    accounts: &[AnthropicAccount],
    store: &dyn CredentialStore,
    live_owner: Option<&str>,
) -> Result<()> {
    let storage = |error: AppError| error.for_account(AccountFailure::StorageUnavailable);
    for holder in accounts {
        let holder_dir = holder.config_dir();
        let Some(blob) = store.read_named(&holder_dir).map_err(storage)? else {
            continue;
        };
        let Some(owner) = blob_account_uuid(&blob)
            .as_deref()
            .and_then(|uuid| owner_of(uuid, accounts))
        else {
            continue;
        };
        if owner.label == holder.label {
            continue;
        }
        let owner_dir = owner.config_dir();
        match store.read_named(&owner_dir).map_err(storage)? {
            // The copy an earlier move could not delete.
            Some(owned) if owned == blob => store.delete_named(&holder_dir).map_err(storage)?,
            Some(_) => {}
            None if live_owner == Some(owner.label.as_str()) => {}
            None => {
                store.write_named(&owner_dir, &blob).map_err(storage)?;
                // A failure here is not undone: the credential is already
                // with its owner, and undoing could lose the last copy.
                store.delete_named(&holder_dir).map_err(storage)?;
            }
        }
    }
    Ok(())
}

/// Register the login plain `claude` is signed into as `account`, without a
/// new sign-in.
///
/// Only the identity marker in the account's own `CLAUDE_CONFIG_DIR` is
/// written: the rotating credential stays in the default slot alone, and the
/// first `switch` away from this account captures it into the account's named
/// slot like any other outgoing login. Signing in again instead would mint a
/// second, independent grant for the same account.
pub fn adopt_current(
    home_claude_json: &Path,
    accounts: &[AnthropicAccount],
    account: &AnthropicAccount,
    store: &dyn CredentialStore,
) -> Result<()> {
    // The same lock as `switch_cli_account`, so an adoption and a switch
    // cannot interleave and both see the same live login.
    let lock_path = switch_lock_path(home_claude_json);
    let _lock = crate::cache::acquire_lock(&lock_path, Duration::from_secs(2))?;

    // The login adopted is the one in the default slot, named by its own
    // credential.
    let live_uuid = store
        .read_default()?
        .ok_or_else(|| {
            AppError::Credentials(
                "the default Claude Code credential slot is empty; a Claude Code sign-in is needed, which this app cannot do".into(),
            )
        })
        .and_then(|blob| {
            blob_account_uuid(&blob).ok_or_else(|| {
                AppError::Credentials("the live `claude` login records no account uuid".into())
            })
        })?;
    if let Some(owner) = accounts.iter().find(|other| {
        other.label != account.label && account_identity(other).as_deref() == Some(&live_uuid)
    }) {
        return Err(AppError::Credentials(format!(
            "the live `claude` login already belongs to account {:?}",
            owner.label
        )));
    }
    if store.read_named(&account.config_dir())?.is_some() {
        return Err(AppError::Credentials(format!(
            "{:?} already has its own Claude login; adopting would leave two copies of one \
             account",
            account.label
        )));
    }
    // The home marker only adds display fields, and only while it describes
    // this same login.
    let live = oauth_account_in(home_claude_json)
        .filter(|marker| uuid_of(marker) == Some(live_uuid.as_str()))
        .unwrap_or_else(|| serde_json::json!({ "accountUuid": live_uuid }));
    let marker = marker_path(&account.config_dir());
    let existing = read_optional(&marker)?;
    let merged = merge_oauth_account(existing.as_deref().unwrap_or_default(), Some(&live))?;
    crate::cache::atomic_write(&marker, &merged)
}

struct RollbackState<'a> {
    target: &'a AnthropicAccount,
    target_blob: &'a str,
    outgoing: Option<&'a AnthropicAccount>,
    original_outgoing_named: Option<&'a str>,
    original_default: Option<&'a str>,
    /// The home marker and what it held, once the switch has rewritten it.
    marker: Option<(&'a Path, Option<&'a [u8]>)>,
}

fn rollback_switch(store: &dyn CredentialStore, state: &RollbackState<'_>) -> Result<()> {
    let mut failures = Vec::new();
    if let Err(error) = restore_default(store, state.original_default) {
        failures.push(format!("default credential: {error}"));
    }
    // Recreate it unconditionally: a failed Keychain delete can still have
    // removed the item before returning its error.
    if let Err(error) = store.write_named(&state.target.config_dir(), state.target_blob) {
        failures.push(format!("target credential: {error}"));
    }
    if let Some(account) = state.outgoing
        && let Err(error) =
            restore_named(store, &account.config_dir(), state.original_outgoing_named)
    {
        failures.push(format!("outgoing credential: {error}"));
    }
    if let Some((path, original)) = state.marker
        && let Err(error) = restore_marker(path, original)
    {
        failures.push(format!("identity marker: {error}"));
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(AppError::Other(failures.join("; ")))
    }
}

fn restore_default(store: &dyn CredentialStore, original: Option<&str>) -> Result<()> {
    match original {
        Some(blob) => store.write_default(blob),
        None => store.delete_default(),
    }
}

fn restore_named(store: &dyn CredentialStore, path: &Path, original: Option<&str>) -> Result<()> {
    match original {
        Some(blob) => store.write_named(path, blob),
        None => store.delete_named(path),
    }
}

fn restore_marker(path: &Path, original: Option<&[u8]>) -> Result<()> {
    if let Some(bytes) = original {
        crate::cache::atomic_write(path, bytes)
    } else {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(AppError::io_at(path, error)),
        }
    }
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(AppError::io_at(path, error)),
    }
}

fn with_rollback(error: AppError, rollback: Result<()>) -> AppError {
    match rollback {
        Ok(()) => error,
        Err(rollback) => AppError::Other(format!(
            "{error}; automatic rollback was incomplete: {rollback}"
        ))
        .for_account(AccountFailure::RecoveryRequired),
    }
}

fn oauth_account_in(claude_json: &Path) -> Option<Value> {
    let bytes = std::fs::read(claude_json).ok()?;
    let document: Value = serde_json::from_slice(&bytes).ok()?;
    document
        .get("oauthAccount")
        .filter(|v| v.is_object())
        .cloned()
}

impl CredentialStore for KeychainStore {
    fn read_default(&self) -> Result<Option<String>> {
        super::keychain::read_raw()
    }

    fn write_default(&self, blob: &str) -> Result<()> {
        super::keychain::write_raw(blob)
    }

    fn delete_default(&self) -> Result<()> {
        super::keychain::delete_raw()
    }

    fn read_named(&self, config_dir: &Path) -> Result<Option<String>> {
        super::keychain::read_raw_for(config_dir)
    }

    fn write_named(&self, config_dir: &Path, blob: &str) -> Result<()> {
        super::keychain::write_raw_for(config_dir, blob)
    }

    fn delete_named(&self, config_dir: &Path) -> Result<()> {
        super::keychain::delete_raw_for(config_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    const WORK_SAVED: &str =
        r#"{"claudeAiOauth":{"accessToken":"work-saved","accountUuid":"uuid-work"}}"#;
    const PERSONAL_LIVE: &str =
        r#"{"claudeAiOauth":{"accessToken":"personal-live","accountUuid":"uuid-personal"}}"#;
    const PERSONAL_STALE: &str =
        r#"{"claudeAiOauth":{"accessToken":"personal-stale","accountUuid":"uuid-personal"}}"#;

    #[derive(Default)]
    struct FakeStore {
        default: RefCell<Option<String>>,
        named: RefCell<BTreeMap<PathBuf, String>>,
        fail_delete_named: RefCell<Option<PathBuf>>,
    }

    impl CredentialStore for FakeStore {
        fn read_default(&self) -> Result<Option<String>> {
            Ok(self.default.borrow().clone())
        }
        fn write_default(&self, blob: &str) -> Result<()> {
            *self.default.borrow_mut() = Some(blob.to_string());
            Ok(())
        }
        fn delete_default(&self) -> Result<()> {
            *self.default.borrow_mut() = None;
            Ok(())
        }
        fn read_named(&self, config_dir: &Path) -> Result<Option<String>> {
            Ok(self.named.borrow().get(config_dir).cloned())
        }
        fn write_named(&self, config_dir: &Path, blob: &str) -> Result<()> {
            self.named
                .borrow_mut()
                .insert(config_dir.to_path_buf(), blob.to_string());
            Ok(())
        }
        fn delete_named(&self, config_dir: &Path) -> Result<()> {
            self.named.borrow_mut().remove(config_dir);
            if self.fail_delete_named.borrow().as_deref() == Some(config_dir) {
                Err(AppError::Other("injected delete failure".into()))
            } else {
                Ok(())
            }
        }
    }

    fn write(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    fn marker(uuid: &str, email: &str) -> String {
        format!(r#"{{"oauthAccount":{{"accountUuid":"{uuid}","emailAddress":"{email}"}}}}"#)
    }

    struct Fixture {
        _root: tempfile::TempDir,
        home: PathBuf,
        accounts: Vec<AnthropicAccount>,
        store: FakeStore,
    }

    /// Two accounts, `work` and `personal`; the CLI is signed into `personal`.
    fn fixture() -> Fixture {
        let root = tempfile::TempDir::new().unwrap();
        let home = root.path().join("home").join(CLAUDE_JSON);
        write(&home, &marker("uuid-personal", "me@personal.test"));

        let accounts: Vec<AnthropicAccount> = ["work", "personal"]
            .iter()
            .map(|label| AnthropicAccount {
                label: (*label).to_string(),
                credentials_path: root
                    .path()
                    .join("accounts")
                    .join(label)
                    .join(".credentials.json"),
            })
            .collect();
        write(
            &accounts[0].config_dir().join(CLAUDE_JSON),
            &marker("uuid-work", "me@work.test"),
        );
        write(
            &accounts[1].config_dir().join(CLAUDE_JSON),
            &marker("uuid-personal", "me@personal.test"),
        );

        let store = FakeStore::default();
        *store.default.borrow_mut() = Some(PERSONAL_LIVE.into());
        store
            .named
            .borrow_mut()
            .insert(accounts[0].config_dir(), WORK_SAVED.into());
        store
            .named
            .borrow_mut()
            .insert(accounts[1].config_dir(), PERSONAL_STALE.into());

        Fixture {
            _root: root,
            home,
            accounts,
            store,
        }
    }

    /// A credential as Claude Code stores it: the account it was issued to
    /// sits beside the tokens.
    fn blob(uuid: &str, secret: &str) -> String {
        format!(
            r#"{{"claudeAiOauth":{{"accessToken":"{secret}","refreshToken":"{secret}-refresh","expiresAt":1,"accountUuid":"{uuid}"}}}}"#
        )
    }

    #[test]
    fn a_stale_marker_does_not_decide_whose_login_the_default_slot_holds() {
        // The home marker still names `personal`, but the default slot holds
        // `work`'s login and `personal`'s own credential is its saved one.
        let f = fixture();
        let work_live = blob("uuid-work", "work-live");
        let personal_saved = blob("uuid-personal", "personal-saved");
        *f.store.default.borrow_mut() = Some(work_live.clone());
        f.store
            .named
            .borrow_mut()
            .remove(&f.accounts[0].config_dir());
        f.store
            .named
            .borrow_mut()
            .insert(f.accounts[1].config_dir(), personal_saved.clone());

        let outcome = switch_cli_account(
            &f.home,
            &f.accounts,
            "personal",
            CliSwitchOpts::default(),
            &f.store,
        )
        .unwrap();

        assert_eq!(
            outcome,
            CliSwitchOutcome::Switched {
                outgoing: Some("work".into())
            }
        );
        assert_eq!(f.store.default.borrow().as_deref(), Some(&*personal_saved));
        assert_eq!(
            f.store.named.borrow().get(&f.accounts[0].config_dir()),
            Some(&work_live)
        );
        assert!(
            !f.store
                .named
                .borrow()
                .contains_key(&f.accounts[1].config_dir())
        );
    }

    /// A third account beside the fixture's two, with its own saved login.
    fn with_spare(f: &mut Fixture) -> AnthropicAccount {
        let spare = AnthropicAccount {
            label: "spare".into(),
            credentials_path: f
                .home
                .parent()
                .unwrap()
                .join("spare")
                .join(".credentials.json"),
        };
        write(
            &spare.config_dir().join(CLAUDE_JSON),
            &marker("uuid-spare", "me@spare.test"),
        );
        f.store
            .named
            .borrow_mut()
            .insert(spare.config_dir(), blob("uuid-spare", "spare-saved"));
        f.accounts.push(spare.clone());
        spare
    }

    fn switch(f: &Fixture, label: &str) -> Result<CliSwitchOutcome> {
        switch_cli_account(
            &f.home,
            &f.accounts,
            label,
            CliSwitchOpts::default(),
            &f.store,
        )
    }

    fn failure(error: &AppError) -> Option<AccountFailure> {
        match error {
            AppError::Account { kind, .. } => Some(*kind),
            _ => None,
        }
    }

    #[test]
    fn the_outgoing_login_is_saved_under_the_account_it_was_issued_to() {
        // The marker names `personal` while the default slot holds `work`'s
        // login. Leaving for a third account must not file it under `personal`.
        let mut f = fixture();
        let spare = with_spare(&mut f);
        let work_live = blob("uuid-work", "work-live");
        *f.store.default.borrow_mut() = Some(work_live.clone());
        f.store
            .named
            .borrow_mut()
            .remove(&f.accounts[0].config_dir());

        let outcome = switch(&f, "spare").unwrap();

        assert_eq!(
            outcome,
            CliSwitchOutcome::Switched {
                outgoing: Some("work".into())
            }
        );
        let named = f.store.named.borrow();
        assert_eq!(named.get(&f.accounts[0].config_dir()), Some(&work_live));
        assert_eq!(named[&f.accounts[1].config_dir()], PERSONAL_STALE);
        assert!(!named.contains_key(&spare.config_dir()));
    }

    #[test]
    fn a_login_that_names_no_account_is_never_saved_under_a_guess() {
        let f = fixture();
        *f.store.default.borrow_mut() = Some("opaque-live".into());
        let before = std::fs::read(&f.home).unwrap();

        let error = switch(&f, "work").unwrap_err();

        assert_eq!(
            failure(&error),
            Some(AccountFailure::UnidentifiedLogin),
            "{error}"
        );
        assert_eq!(f.store.default.borrow().as_deref(), Some("opaque-live"));
        let named = f.store.named.borrow();
        assert_eq!(named[&f.accounts[0].config_dir()], WORK_SAVED);
        assert_eq!(named[&f.accounts[1].config_dir()], PERSONAL_STALE);
        assert_eq!(std::fs::read(&f.home).unwrap(), before);
    }

    /// `personal`'s slot holds `work`'s login and `work` has none, with `spare`
    /// live: what a switch that believed a stale marker left behind.
    fn stray_work_login(f: &mut Fixture) -> String {
        with_spare(f);
        let work_login = blob("uuid-work", "work-login");
        *f.store.default.borrow_mut() = Some(blob("uuid-spare", "spare-live"));
        let mut named = f.store.named.borrow_mut();
        named.remove(&f.accounts[0].config_dir());
        named.remove(&f.accounts[2].config_dir());
        named.insert(f.accounts[1].config_dir(), work_login.clone());
        work_login
    }

    #[test]
    fn a_credential_saved_in_another_accounts_slot_goes_back_to_its_owner() {
        let mut f = fixture();
        let work_login = stray_work_login(&mut f);

        // Asking for the account already in use changes no login, and still
        // puts the stray credential back.
        assert_eq!(
            switch(&f, "spare").unwrap(),
            CliSwitchOutcome::AlreadyActive
        );
        {
            let named = f.store.named.borrow();
            assert_eq!(named.get(&f.accounts[0].config_dir()), Some(&work_login));
            assert!(!named.contains_key(&f.accounts[1].config_dir()));
        }

        assert_eq!(
            switch(&f, "work").unwrap(),
            CliSwitchOutcome::Switched {
                outgoing: Some("spare".into())
            }
        );
        assert_eq!(f.store.default.borrow().as_deref(), Some(&*work_login));
    }

    #[test]
    fn a_failed_move_never_loses_the_credential() {
        let mut f = fixture();
        let work_login = stray_work_login(&mut f);
        *f.store.fail_delete_named.borrow_mut() = Some(f.accounts[1].config_dir());

        let error = switch(&f, "spare").unwrap_err();

        assert_eq!(
            failure(&error),
            Some(AccountFailure::StorageUnavailable),
            "{error}"
        );
        // It reached its owner before the failure and is not taken back.
        assert_eq!(
            f.store.named.borrow().get(&f.accounts[0].config_dir()),
            Some(&work_login)
        );
    }

    #[test]
    fn a_copy_left_by_a_failed_move_is_removed_by_the_next_switch() {
        let mut f = fixture();
        let work_login = stray_work_login(&mut f);
        f.store
            .named
            .borrow_mut()
            .insert(f.accounts[0].config_dir(), work_login.clone());

        assert_eq!(
            switch(&f, "spare").unwrap(),
            CliSwitchOutcome::AlreadyActive
        );
        let named = f.store.named.borrow();
        assert_eq!(named.get(&f.accounts[0].config_dir()), Some(&work_login));
        assert!(!named.contains_key(&f.accounts[1].config_dir()));
    }

    #[test]
    fn a_saved_credential_that_names_no_account_is_never_installed() {
        let f = fixture();
        f.store
            .named
            .borrow_mut()
            .insert(f.accounts[0].config_dir(), "opaque-saved".into());
        let before = std::fs::read(&f.home).unwrap();

        let error = switch(&f, "work").unwrap_err();

        assert_eq!(
            failure(&error),
            Some(AccountFailure::UnidentifiedLogin),
            "{error}"
        );
        assert_eq!(f.store.default.borrow().as_deref(), Some(PERSONAL_LIVE));
        let named = f.store.named.borrow();
        assert_eq!(named[&f.accounts[0].config_dir()], "opaque-saved");
        assert_eq!(named[&f.accounts[1].config_dir()], PERSONAL_STALE);
        assert_eq!(std::fs::read(&f.home).unwrap(), before);
    }

    #[test]
    fn a_saved_credential_that_names_no_account_is_not_deleted_as_a_duplicate() {
        let f = fixture();
        f.store
            .named
            .borrow_mut()
            .insert(f.accounts[1].config_dir(), "opaque-saved".into());

        assert_eq!(
            switch(&f, "personal").unwrap(),
            CliSwitchOutcome::AlreadyActive
        );
        assert_eq!(
            f.store.named.borrow()[&f.accounts[1].config_dir()],
            "opaque-saved"
        );
    }

    #[test]
    fn a_refused_switch_still_returns_a_stray_credential_to_its_owner() {
        // `spare` has no saved login, so asking for it is refused. The stray
        // `work` login is put back all the same.
        let mut f = fixture();
        let work_login = stray_work_login(&mut f);
        *f.store.default.borrow_mut() = Some(PERSONAL_LIVE.into());

        let error = switch(&f, "spare").unwrap_err();

        assert_eq!(
            failure(&error),
            Some(AccountFailure::MissingLogin),
            "{error}"
        );
        assert_eq!(f.store.default.borrow().as_deref(), Some(PERSONAL_LIVE));
        let named = f.store.named.borrow();
        assert_eq!(named.get(&f.accounts[0].config_dir()), Some(&work_login));
        assert!(!named.contains_key(&f.accounts[1].config_dir()));
    }

    #[test]
    fn the_home_file_keeps_what_was_written_to_it_during_the_switch() {
        // A Claude Code session saves its own state while the Keychain work
        // is under way: the store's first write stands in for that moment.
        struct Racing<'a> {
            inner: &'a FakeStore,
            home: &'a Path,
        }
        impl CredentialStore for Racing<'_> {
            fn read_default(&self) -> Result<Option<String>> {
                self.inner.read_default()
            }
            fn write_default(&self, blob: &str) -> Result<()> {
                let mut document: Value =
                    serde_json::from_slice(&std::fs::read(self.home).unwrap()).unwrap();
                document["numStartups"] = 7.into();
                std::fs::write(self.home, serde_json::to_vec(&document).unwrap()).unwrap();
                self.inner.write_default(blob)
            }
            fn delete_default(&self) -> Result<()> {
                self.inner.delete_default()
            }
            fn read_named(&self, dir: &Path) -> Result<Option<String>> {
                self.inner.read_named(dir)
            }
            fn write_named(&self, dir: &Path, blob: &str) -> Result<()> {
                self.inner.write_named(dir, blob)
            }
            fn delete_named(&self, dir: &Path) -> Result<()> {
                self.inner.delete_named(dir)
            }
        }
        let f = fixture();
        let store = Racing {
            inner: &f.store,
            home: &f.home,
        };

        switch_cli_account(
            &f.home,
            &f.accounts,
            "work",
            CliSwitchOpts::default(),
            &store,
        )
        .unwrap();

        let document: Value = serde_json::from_slice(&std::fs::read(&f.home).unwrap()).unwrap();
        assert_eq!(document["numStartups"], 7);
        assert_eq!(document["oauthAccount"]["accountUuid"], "uuid-work");
    }

    #[test]
    fn a_target_slot_holding_another_accounts_login_is_refused() {
        // `personal` is live, so its copy in `work`'s slot has nowhere to go.
        let f = fixture();
        let stray = blob("uuid-personal", "personal-copy");
        f.store
            .named
            .borrow_mut()
            .insert(f.accounts[0].config_dir(), stray.clone());
        let before = std::fs::read(&f.home).unwrap();

        let error = switch(&f, "work").unwrap_err();

        assert_eq!(
            failure(&error),
            Some(AccountFailure::WrongAccount),
            "{error}"
        );
        assert_eq!(f.store.default.borrow().as_deref(), Some(PERSONAL_LIVE));
        let named = f.store.named.borrow();
        assert_eq!(named[&f.accounts[0].config_dir()], stray);
        assert_eq!(named[&f.accounts[1].config_dir()], PERSONAL_STALE);
        assert_eq!(std::fs::read(&f.home).unwrap(), before);
    }

    #[test]
    fn a_foreign_credential_in_the_active_accounts_slot_is_not_a_duplicate() {
        let f = fixture();
        let stranger = blob("uuid-stranger", "stranger-saved");
        f.store
            .named
            .borrow_mut()
            .insert(f.accounts[1].config_dir(), stranger.clone());

        assert_eq!(
            switch(&f, "personal").unwrap(),
            CliSwitchOutcome::AlreadyActive
        );
        assert_eq!(
            f.store.named.borrow()[&f.accounts[1].config_dir()],
            stranger
        );
    }

    #[test]
    fn a_label_keeps_its_account_after_a_sign_in_rewrites_its_marker() {
        let f = fixture();
        record_identities(&f.accounts);
        // A sign-in in `work`'s directory that the browser authorized as
        // `personal`.
        write(
            &f.accounts[0].config_dir().join(CLAUDE_JSON),
            &marker("uuid-personal", "me@personal.test"),
        );

        assert_eq!(
            account_identity(&f.accounts[0]).as_deref(),
            Some("uuid-work")
        );
        assert_eq!(
            account_email(&f.accounts[0]).as_deref(),
            Some("me@work.test")
        );
        assert_eq!(
            resolve_active_label(Some(PERSONAL_LIVE), &f.accounts).as_deref(),
            Some("personal")
        );
        // The home marker written by a switch names the recorded account too.
        assert_eq!(
            switch(&f, "work").unwrap(),
            CliSwitchOutcome::Switched {
                outgoing: Some("personal".into())
            }
        );
        assert_eq!(account_uuid_in(&f.home).as_deref(), Some("uuid-work"));
    }

    #[test]
    fn two_labels_on_one_account_resolve_to_neither() {
        let f = fixture();
        write(
            &f.accounts[0].config_dir().join(CLAUDE_JSON),
            &marker("uuid-personal", "me@personal.test"),
        );

        assert_eq!(resolve_active_label(Some(PERSONAL_LIVE), &f.accounts), None);
        record_identities(&f.accounts);
        assert!(!identity_path(&f.accounts[0].config_dir()).exists());
        assert!(!identity_path(&f.accounts[1].config_dir()).exists());
        assert_eq!(
            failure(&switch(&f, "work").unwrap_err()),
            Some(AccountFailure::InvalidState)
        );
    }

    #[test]
    fn the_live_login_resolves_to_its_label() {
        let f = fixture();
        assert_eq!(
            resolve_active_label(Some(PERSONAL_LIVE), &f.accounts).as_deref(),
            Some("personal")
        );
        assert_eq!(
            account_email(&f.accounts[1]).as_deref(),
            Some("me@personal.test")
        );
    }

    #[test]
    fn adopting_marks_the_live_login_and_leaves_the_credential_alone() {
        let f = fixture();
        write(&f.home, &marker("uuid-main", "me@main.test"));
        let main_live = blob("uuid-main", "main-live");
        *f.store.default.borrow_mut() = Some(main_live.clone());
        let main = AnthropicAccount {
            label: "main".into(),
            credentials_path: f
                .home
                .parent()
                .unwrap()
                .join("main")
                .join(".credentials.json"),
        };
        let mut accounts = f.accounts.clone();
        accounts.push(main.clone());
        adopt_current(&f.home, &accounts, &main, &f.store).unwrap();
        assert_eq!(
            resolve_active_label(Some(&main_live), &accounts).as_deref(),
            Some("main")
        );
        assert_eq!(account_email(&main).as_deref(), Some("me@main.test"));
        assert_eq!(f.store.read_named(&main.config_dir()).unwrap(), None);
        assert_eq!(f.store.read_default().unwrap(), Some(main_live));
    }

    #[test]
    fn adopting_refuses_a_login_another_account_owns() {
        let f = fixture();
        let main = AnthropicAccount {
            label: "main".into(),
            credentials_path: f
                .home
                .parent()
                .unwrap()
                .join("main")
                .join(".credentials.json"),
        };
        let error = adopt_current(&f.home, &f.accounts, &main, &f.store).unwrap_err();
        assert!(error.to_string().contains("\"personal\""), "{error}");
    }

    #[test]
    fn adopting_refuses_an_account_with_its_own_login() {
        let f = fixture();
        *f.store.default.borrow_mut() = Some(blob("uuid-main", "main-live"));
        let error = adopt_current(&f.home, &f.accounts, &f.accounts[0], &f.store).unwrap_err();
        assert!(error.to_string().contains("two copies"), "{error}");
    }

    #[test]
    fn an_unmanaged_login_resolves_to_nothing() {
        let f = fixture();
        let stranger = blob("uuid-stranger", "stranger-live");
        assert_eq!(resolve_active_label(Some(&stranger), &f.accounts), None);
    }

    #[test]
    fn an_empty_or_unidentified_default_slot_resolves_to_nothing() {
        let f = fixture();
        assert_eq!(resolve_active_label(None, &f.accounts), None);
        assert_eq!(resolve_active_label(Some("opaque"), &f.accounts), None);
        let unnamed = r#"{"claudeAiOauth":{"accessToken":"x"}}"#;
        assert_eq!(resolve_active_label(Some(unnamed), &f.accounts), None);
    }

    #[test]
    fn a_missing_marker_file_is_not_an_error() {
        let f = fixture();
        std::fs::remove_file(&f.home).unwrap();
        assert_eq!(account_uuid_in(&f.home), None);
    }

    #[test]
    fn switching_captures_the_outgoing_credential_first() {
        let f = fixture();

        let outcome = switch_cli_account(
            &f.home,
            &f.accounts,
            "work",
            CliSwitchOpts::default(),
            &f.store,
        )
        .unwrap();

        assert_eq!(
            outcome,
            CliSwitchOutcome::Switched {
                outgoing: Some("personal".into())
            }
        );
        // The freshest lineage was written back into personal's own slot before
        // the default slot was overwritten.
        assert_eq!(
            f.store.named.borrow()[&f.accounts[1].config_dir()],
            PERSONAL_LIVE
        );
        assert_eq!(f.store.default.borrow().as_deref(), Some(WORK_SAVED));
        assert!(
            !f.store
                .named
                .borrow()
                .contains_key(&f.accounts[0].config_dir()),
            "the active credential must be moved, not left as a rotating-token copy"
        );
        assert_eq!(
            resolve_active_label(f.store.default.borrow().as_deref(), &f.accounts).as_deref(),
            Some("work")
        );
        assert_eq!(account_uuid_in(&f.home).as_deref(), Some("uuid-work"));
    }

    #[test]
    fn switching_is_idempotent() {
        let f = fixture();
        let outcome = switch_cli_account(
            &f.home,
            &f.accounts,
            "personal",
            CliSwitchOpts::default(),
            &f.store,
        )
        .unwrap();
        assert_eq!(outcome, CliSwitchOutcome::RemovedDuplicate);
        assert_eq!(f.store.default.borrow().as_deref(), Some(PERSONAL_LIVE));
        assert!(
            !f.store
                .named
                .borrow()
                .contains_key(&f.accounts[1].config_dir())
        );

        let second = switch_cli_account(
            &f.home,
            &f.accounts,
            "personal",
            CliSwitchOpts::default(),
            &f.store,
        )
        .unwrap();
        assert_eq!(second, CliSwitchOutcome::AlreadyActive);
    }

    #[test]
    fn an_active_marker_with_an_empty_default_slot_is_repaired() {
        let f = fixture();
        *f.store.default.borrow_mut() = None;

        let outcome = switch_cli_account(
            &f.home,
            &f.accounts,
            "personal",
            CliSwitchOpts::default(),
            &f.store,
        )
        .unwrap();

        assert_eq!(outcome, CliSwitchOutcome::RepairedActive);
        assert_eq!(f.store.default.borrow().as_deref(), Some(PERSONAL_STALE));
        assert!(
            !f.store
                .named
                .borrow()
                .contains_key(&f.accounts[1].config_dir())
        );
    }

    #[test]
    fn a_dry_run_validates_without_writing() {
        let f = fixture();
        let before = std::fs::read(&f.home).unwrap();

        let outcome = switch_cli_account(
            &f.home,
            &f.accounts,
            "work",
            CliSwitchOpts {
                dry_run: true,
                ..CliSwitchOpts::default()
            },
            &f.store,
        )
        .unwrap();

        assert_eq!(
            outcome,
            CliSwitchOutcome::WouldSwitch {
                outgoing: Some("personal".into())
            }
        );
        assert_eq!(f.store.default.borrow().as_deref(), Some(PERSONAL_LIVE));
        assert_eq!(std::fs::read(&f.home).unwrap(), before);
    }

    #[test]
    fn an_unmanaged_live_login_is_refused_without_force() {
        let f = fixture();
        let stranger = blob("uuid-stranger", "stranger-live");
        *f.store.default.borrow_mut() = Some(stranger.clone());

        let error = switch_cli_account(
            &f.home,
            &f.accounts,
            "work",
            CliSwitchOpts::default(),
            &f.store,
        )
        .unwrap_err();
        assert!(error.to_string().contains("not managed here"), "{error}");
        assert_eq!(f.store.default.borrow().as_deref(), Some(&*stranger));

        switch_cli_account(
            &f.home,
            &f.accounts,
            "work",
            CliSwitchOpts {
                force: true,
                ..CliSwitchOpts::default()
            },
            &f.store,
        )
        .unwrap();
        assert_eq!(f.store.default.borrow().as_deref(), Some(WORK_SAVED));
        assert!(
            !f.store
                .named
                .borrow()
                .contains_key(&f.accounts[0].config_dir())
        );
    }

    #[test]
    fn an_empty_default_slot_does_not_require_force() {
        let f = fixture();
        *f.store.default.borrow_mut() = None;
        std::fs::remove_file(&f.home).unwrap();

        switch_cli_account(
            &f.home,
            &f.accounts,
            "work",
            CliSwitchOpts::default(),
            &f.store,
        )
        .unwrap();

        assert_eq!(f.store.default.borrow().as_deref(), Some(WORK_SAVED));
        assert_eq!(account_uuid_in(&f.home).as_deref(), Some("uuid-work"));
    }

    #[test]
    fn a_target_without_an_identity_marker_changes_nothing() {
        let f = fixture();
        std::fs::remove_file(f.accounts[0].config_dir().join(CLAUDE_JSON)).unwrap();
        let before = std::fs::read(&f.home).unwrap();

        let error = switch_cli_account(
            &f.home,
            &f.accounts,
            "work",
            CliSwitchOpts::default(),
            &f.store,
        )
        .unwrap_err();

        assert!(error.to_string().contains("identity marker"), "{error}");
        assert_eq!(f.store.default.borrow().as_deref(), Some(PERSONAL_LIVE));
        assert_eq!(std::fs::read(&f.home).unwrap(), before);
    }

    #[test]
    fn a_late_failure_restores_every_credential_slot_and_marker() {
        let f = fixture();
        let before_marker = std::fs::read(&f.home).unwrap();
        *f.store.fail_delete_named.borrow_mut() = Some(f.accounts[0].config_dir());

        let error = switch_cli_account(
            &f.home,
            &f.accounts,
            "work",
            CliSwitchOpts::default(),
            &f.store,
        )
        .unwrap_err();

        assert!(
            error.to_string().contains("injected delete failure"),
            "{error}"
        );
        assert_eq!(f.store.default.borrow().as_deref(), Some(PERSONAL_LIVE));
        assert_eq!(
            f.store.named.borrow()[&f.accounts[0].config_dir()],
            WORK_SAVED
        );
        assert_eq!(
            f.store.named.borrow()[&f.accounts[1].config_dir()],
            PERSONAL_STALE
        );
        assert_eq!(std::fs::read(&f.home).unwrap(), before_marker);
    }

    #[test]
    fn switching_to_an_account_that_never_signed_in_changes_nothing() {
        let f = fixture();
        f.store
            .named
            .borrow_mut()
            .remove(&f.accounts[0].config_dir());

        let error = switch_cli_account(
            &f.home,
            &f.accounts,
            "work",
            CliSwitchOpts::default(),
            &f.store,
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("never been signed in"),
            "{error}"
        );
        assert_eq!(f.store.default.borrow().as_deref(), Some(PERSONAL_LIVE));
    }

    #[test]
    fn merging_the_marker_preserves_unrelated_state() {
        let existing = br#"{"firstStartTime":"2026-01-01","oauthAccount":{"accountUuid":"old"},
            "projects":{"/tmp/x":{"allowedTools":[]}}}"#;
        let replacement = serde_json::json!({"accountUuid": "new", "emailAddress": "a@b.test"});

        let bytes = merge_oauth_account(existing, Some(&replacement)).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["oauthAccount"]["accountUuid"], "new");
        assert_eq!(value["firstStartTime"], "2026-01-01");
        assert!(value["projects"]["/tmp/x"].is_object());
    }

    #[test]
    fn merging_no_marker_clears_a_stale_one() {
        let existing = br#"{"oauthAccount":{"accountUuid":"old"},"autoUpdates":true}"#;
        let bytes = merge_oauth_account(existing, None).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(value.get("oauthAccount").is_none());
        assert_eq!(value["autoUpdates"], true);
    }

    #[test]
    fn merging_into_an_absent_file_starts_a_fresh_document() {
        let replacement = serde_json::json!({"accountUuid": "new"});
        let bytes = merge_oauth_account(b"", Some(&replacement)).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["oauthAccount"]["accountUuid"], "new");
    }
}
