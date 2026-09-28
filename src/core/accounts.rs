//! Account switching independent of terminal input and presentation.
//! Credential transactions and rollback remain in each provider's service.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::anthropic::cli_account::{self, CliSwitchOpts, CliSwitchOutcome, CredentialStore};
use crate::config::Config;
use crate::error::{AccountFailure, AppError};
use crate::openai::account::{self as codex, SwitchOpts, SwitchOutcome};

pub(crate) const VERSION: u8 = 1;
pub(crate) const MAX_LABEL_UNITS: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Provider {
    ClaudeCli,
    Codex,
}

impl Provider {
    pub(crate) fn from_vendor(vendor: &str) -> Option<Self> {
        match vendor {
            "anthropic" => Some(Self::ClaudeCli),
            "openai" => Some(Self::Codex),
            _ => None,
        }
    }

    pub(crate) fn cache_vendor(self) -> &'static str {
        match self {
            Self::ClaudeCli => "anthropic",
            Self::Codex => "openai",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SwitchRequest {
    version: u8,
    pub(crate) provider: Provider,
    label: String,
}

impl SwitchRequest {
    pub(crate) fn new(provider: Provider, label: String) -> Result<Self, Failure> {
        let request = Self {
            version: VERSION,
            provider,
            label,
        };
        request.validate()?;
        Ok(request)
    }

    pub(crate) fn validate(&self) -> Result<(), Failure> {
        if self.version != VERSION
            || self.label.encode_utf16().count() > MAX_LABEL_UNITS
            || crate::config::validate_account_label(&self.label).is_err()
        {
            return Err(Failure::InvalidRequest);
        }
        Ok(())
    }
}

/// Only fixed categories cross the worker boundary. In particular, parser
/// diagnostics and credential-store errors may contain secrets or paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Failure {
    InvalidRequest,
    InvalidConfiguration,
    UnknownAccount,
    CustomDefaultSlot,
    StorageUnavailable,
    LockUnavailable,
    RecoveryRequired,
    UnmanagedLogin,
    MissingLogin,
    InvalidAccountState,
    SwitchFailed,
    WorkerUnavailable,
    InvalidResponse,
}

impl Failure {
    fn from_account_error(error: AppError) -> Self {
        match error {
            AppError::Account { kind, .. } => match kind {
                AccountFailure::RecoveryRequired => Self::RecoveryRequired,
                AccountFailure::UnmanagedLogin => Self::UnmanagedLogin,
                AccountFailure::MissingLogin => Self::MissingLogin,
                AccountFailure::InvalidState => Self::InvalidAccountState,
                AccountFailure::StorageUnavailable => Self::StorageUnavailable,
                AccountFailure::LockUnavailable => Self::LockUnavailable,
            },
            AppError::Io { .. } | AppError::IoBare(_) => Self::StorageUnavailable,
            _ => Self::SwitchFailed,
        }
    }

    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::InvalidRequest => "A solicitação de troca de conta é inválida.",
            Self::InvalidConfiguration => {
                "Não foi possível ler a configuração. Nenhuma troca foi iniciada."
            }
            Self::UnknownAccount => {
                "Esta conta não está mais cadastrada. Atualize a lista de contas."
            }
            Self::CustomDefaultSlot => {
                "A configuração do Codex aponta para um arquivo personalizado. A troca exige a sessão padrão do Codex."
            }
            Self::StorageUnavailable => {
                "Não foi possível acessar o armazenamento das contas. Confira as permissões e o acesso ao Chaves do macOS."
            }
            Self::LockUnavailable => {
                "Não foi possível reservar o acesso às contas. Aguarde outras operações terminarem e confira as permissões."
            }
            Self::RecoveryRequired => {
                "A recuperação automática ficou incompleta. Não faça outra troca. Verifique e recupere o acesso às contas antes de continuar."
            }
            Self::UnmanagedLogin => {
                "A sessão atual não está cadastrada no aplicativo, e o cadastro de contas ainda não está disponível na interface. A troca foi cancelada para preservar seu acesso."
            }
            Self::MissingLogin => {
                "Esta conta está sem uma sessão salva, e conectar contas ainda não está disponível na interface."
            }
            Self::InvalidAccountState => {
                "A identidade salva desta conta está inválida ou duplicada. Confira as contas conectadas antes de trocar."
            }
            Self::SwitchFailed => {
                "Não foi possível concluir a troca. Confira a conta ativa e o armazenamento das contas."
            }
            Self::WorkerUnavailable => "Não foi possível iniciar o processo de troca de conta.",
            Self::InvalidResponse => {
                "Não foi possível confirmar o resultado da troca. Atualize as contas antes de tentar novamente."
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Effect {
    AlreadyActive,
    RemovedDuplicate,
    RepairedActive,
    Switched,
}

impl Effect {
    /// Preserve the existing cache policy: only a real account change drops
    /// the unnamed slot's cache. Named caches keep their identity.
    pub(crate) fn invalidates_default_cache(self) -> bool {
        self == Self::Switched
    }
}

/// All paths and credential access are injected. No default-path resolution,
/// CLI parsing, environment mutation or presentation occurs in this service.
pub(crate) fn switch_at(
    request: &SwitchRequest,
    config: &Config,
    claude_marker: &Path,
    codex_default: &Path,
    store: &dyn CredentialStore,
) -> Result<Effect, Failure> {
    request.validate()?;
    match request.provider {
        Provider::ClaudeCli => {
            let accounts = config.anthropic.all_accounts();
            if !accounts
                .iter()
                .any(|account| account.label == request.label)
            {
                return Err(Failure::UnknownAccount);
            }
            let outcome = cli_account::switch_cli_account(
                claude_marker,
                &accounts,
                &request.label,
                CliSwitchOpts {
                    force: false,
                    dry_run: false,
                },
                store,
            )
            .map_err(Failure::from_account_error)?;
            match outcome {
                CliSwitchOutcome::AlreadyActive => Ok(Effect::AlreadyActive),
                CliSwitchOutcome::RemovedDuplicate => Ok(Effect::RemovedDuplicate),
                CliSwitchOutcome::RepairedActive => Ok(Effect::RepairedActive),
                CliSwitchOutcome::Switched { .. } => Ok(Effect::Switched),
                CliSwitchOutcome::WouldSwitch { .. } | CliSwitchOutcome::WouldRemoveDuplicate => {
                    Err(Failure::SwitchFailed)
                }
            }
        }
        Provider::Codex => {
            if config
                .openai
                .codex_auth_path
                .as_ref()
                .is_some_and(|path| path != codex_default)
            {
                return Err(Failure::CustomDefaultSlot);
            }
            if !config
                .openai
                .accounts
                .iter()
                .any(|account| account.label == request.label)
            {
                return Err(Failure::UnknownAccount);
            }
            let outcome = codex::switch_account(
                codex_default,
                &config.openai.accounts,
                &request.label,
                SwitchOpts {
                    force: false,
                    dry_run: false,
                },
            )
            .map_err(Failure::from_account_error)?;
            match outcome {
                SwitchOutcome::AlreadyActive => Ok(Effect::AlreadyActive),
                SwitchOutcome::Switched { .. } => Ok(Effect::Switched),
                SwitchOutcome::WouldSwitch { .. } => Err(Failure::SwitchFailed),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OpenAiAccount;
    use crate::error::Result as AppResult;
    use std::cell::{Cell, RefCell};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    struct ForbiddenStore;
    impl CredentialStore for ForbiddenStore {
        fn read_default(&self) -> AppResult<Option<String>> {
            panic!("unexpected credential read")
        }
        fn write_default(&self, _: &str) -> AppResult<()> {
            panic!("unexpected credential write")
        }
        fn delete_default(&self) -> AppResult<()> {
            panic!("unexpected credential deletion")
        }
        fn read_named(&self, _: &Path) -> AppResult<Option<String>> {
            panic!("unexpected credential read")
        }
        fn write_named(&self, _: &Path, _: &str) -> AppResult<()> {
            panic!("unexpected credential write")
        }
        fn delete_named(&self, _: &Path) -> AppResult<()> {
            panic!("unexpected credential deletion")
        }
    }

    #[derive(Default)]
    struct MemoryStore {
        default: RefCell<Option<String>>,
        named: RefCell<BTreeMap<PathBuf, String>>,
        fail_deletion_and_restore: Cell<bool>,
    }

    impl CredentialStore for MemoryStore {
        fn read_default(&self) -> AppResult<Option<String>> {
            Ok(self.default.borrow().clone())
        }
        fn write_default(&self, blob: &str) -> AppResult<()> {
            *self.default.borrow_mut() = Some(blob.into());
            Ok(())
        }
        fn delete_default(&self) -> AppResult<()> {
            *self.default.borrow_mut() = None;
            Ok(())
        }
        fn read_named(&self, path: &Path) -> AppResult<Option<String>> {
            Ok(self.named.borrow().get(path).cloned())
        }
        fn write_named(&self, path: &Path, blob: &str) -> AppResult<()> {
            if self.fail_deletion_and_restore.get() {
                return Err(AppError::Other("fixture-private-error".into()));
            }
            self.named.borrow_mut().insert(path.into(), blob.into());
            Ok(())
        }
        fn delete_named(&self, path: &Path) -> AppResult<()> {
            self.named.borrow_mut().remove(path);
            if self.fail_deletion_and_restore.get() {
                return Err(AppError::Other("fixture-private-error".into()));
            }
            Ok(())
        }
    }

    #[test]
    fn claude_switch_never_forces_a_login_and_moves_the_credential_only_once() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join(".claude.json");
        let named = dir.path().join("work");
        std::fs::create_dir(&named).unwrap();
        std::fs::write(
            named.join(".claude.json"),
            r#"{"oauthAccount":{"accountUuid":"fixture-work","emailAddress":"work@example.test"}}"#,
        )
        .unwrap();
        let mut config = Config::default();
        config
            .anthropic
            .accounts
            .push(crate::config::AnthropicAccount {
                label: "work".into(),
                credentials_path: named.join("unused-fixture.json"),
            });
        let store = MemoryStore::default();
        *store.default.borrow_mut() = Some("unmanaged-live".into());
        store
            .named
            .borrow_mut()
            .insert(named.clone(), "work-saved".into());
        let request = SwitchRequest::new(Provider::ClaudeCli, "work".into()).unwrap();
        let run = || {
            switch_at(
                &request,
                &config,
                &marker,
                &dir.path().join("unused"),
                &store,
            )
        };

        assert_eq!(run(), Err(Failure::UnmanagedLogin));
        assert_eq!(store.default.borrow().as_deref(), Some("unmanaged-live"));
        assert_eq!(store.named.borrow()[&named], "work-saved");
        assert!(!marker.exists());

        // Make the fixture's in-memory default empty. The real Keychain is
        // never read or written by this test.
        *store.default.borrow_mut() = None;
        store.fail_deletion_and_restore.set(true);
        assert_eq!(run(), Err(Failure::RecoveryRequired));
        assert!(store.default.borrow().is_none());
        assert!(store.named.borrow().is_empty());
        assert!(!marker.exists());
        assert_eq!(run(), Err(Failure::MissingLogin));

        // Restore the fixture explicitly after the injected recovery failure.
        store.fail_deletion_and_restore.set(false);
        store
            .named
            .borrow_mut()
            .insert(named.clone(), "work-saved".into());
        assert_eq!(run(), Ok(Effect::Switched));
        assert_eq!(store.default.borrow().as_deref(), Some("work-saved"));
        assert!(store.named.borrow().is_empty());
        assert_eq!(
            cli_account::resolve_active_label(&marker, &config.anthropic.accounts).as_deref(),
            Some("work")
        );
        assert_eq!(run(), Ok(Effect::AlreadyActive));
    }

    #[test]
    fn labels_keep_the_ui_utf16_limit_and_reject_path_or_control_input() {
        for label in ["", "..", "a/b", "a\\b", "a:b", "a\nb", ".fetch.lock"] {
            assert!(SwitchRequest::new(Provider::Codex, label.into()).is_err());
        }
        assert!(SwitchRequest::new(Provider::Codex, "a".repeat(4096)).is_ok());
        assert!(SwitchRequest::new(Provider::Codex, "a".repeat(4097)).is_err());
        assert!(SwitchRequest::new(Provider::Codex, "😀".repeat(2048)).is_ok());
        assert!(SwitchRequest::new(Provider::Codex, "😀".repeat(2049)).is_err());
        assert!(SwitchRequest::new(Provider::Codex, "-work team".into()).is_ok());
        assert!(SwitchRequest::new(Provider::Codex, " ".into()).is_ok());
        assert!(Provider::from_vendor("copilot").is_none());
    }

    #[test]
    fn unknown_accounts_and_custom_codex_slot_touch_no_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("claude-marker.json");
        let default = dir.path().join("auth.json");
        let mut config = Config::default();
        for provider in [Provider::ClaudeCli, Provider::Codex] {
            let request = SwitchRequest::new(provider, "unknown".into()).unwrap();
            assert_eq!(
                switch_at(&request, &config, &marker, &default, &ForbiddenStore),
                Err(Failure::UnknownAccount)
            );
        }
        config.openai.codex_auth_path = Some(dir.path().join("custom.json"));
        let request = SwitchRequest::new(Provider::Codex, "work".into()).unwrap();
        assert_eq!(
            switch_at(&request, &config, &marker, &default, &ForbiddenStore),
            Err(Failure::CustomDefaultSlot)
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn codex_switch_uses_the_transaction_and_never_forces_an_unmanaged_login() {
        let dir = tempfile::tempdir().unwrap();
        let default = dir.path().join("default/auth.json");
        let named = dir.path().join("work/auth.json");
        std::fs::create_dir_all(default.parent().unwrap()).unwrap();
        std::fs::create_dir_all(named.parent().unwrap()).unwrap();
        let auth = |id| {
            serde_json::json!({"tokens":{"account_id":id,"access_token":"fixture-access","refresh_token":"fixture-refresh","id_token":"x.y.z"}}).to_string()
        };
        std::fs::write(&default, auth("unmanaged")).unwrap();
        std::fs::write(&named, auth("work")).unwrap();
        let mut config = Config::default();
        config.openai.accounts.push(OpenAiAccount {
            label: "work".into(),
            codex_auth_path: named.clone(),
        });
        let request = SwitchRequest::new(Provider::Codex, "work".into()).unwrap();
        let run = || {
            switch_at(
                &request,
                &config,
                &dir.path().join("unused-marker"),
                &default,
                &ForbiddenStore,
            )
        };
        assert_eq!(run(), Err(Failure::UnmanagedLogin));
        assert_eq!(
            std::fs::read_to_string(&default).unwrap(),
            auth("unmanaged")
        );
        assert_eq!(std::fs::read_to_string(&named).unwrap(), auth("work"));

        // A new empty default slot permits the existing transaction to move
        // the named account. No user file or Keychain participates.
        let empty_default = dir.path().join("empty/auth.json");
        let switched = switch_at(
            &request,
            &config,
            &dir.path().join("unused-marker"),
            &empty_default,
            &ForbiddenStore,
        )
        .unwrap();
        assert_eq!(switched, Effect::Switched);
        assert!(switched.invalidates_default_cache());
        assert_eq!(
            std::fs::read_to_string(&empty_default).unwrap(),
            auth("work")
        );
        assert!(!named.exists());
        let again = switch_at(
            &request,
            &config,
            &dir.path().join("unused-marker"),
            &empty_default,
            &ForbiddenStore,
        )
        .unwrap();
        assert_eq!(again, Effect::AlreadyActive);
        assert!(!again.invalidates_default_cache());
        assert!(!Effect::RepairedActive.invalidates_default_cache());
        assert!(!Effect::RemovedDuplicate.invalidates_default_cache());
    }
}
