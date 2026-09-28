# Alvos de desenvolvimento do aplicativo visual para macOS com Apple Silicon.
# Nenhum alvo instala o aplicativo, abre a cópia de uso ou reinicia o serviço.

# Também nomeado em build.rs (FRONTEND_DIR) e ci.yml.
FRONTEND_DIR := frontend

.PHONY: build bundle test rust-test node-test frontend-deps frontend-test frontend-typecheck \
	macos-native-test changelog-check lint fmt fmt-check machete smoke

build:
	cargo build --release --locked --bin ai-usagebar-tray

# Gera target/release/AI Usage.app (arm64) assinado por scripts/sign-macos-tray.sh.
bundle:
	./scripts/bundle-macos-app.sh

test: rust-test node-test frontend-test frontend-typecheck

rust-test:
	cargo test --all-targets --locked

# Contratos dos scripts, sem chaveiro, certificado nem aplicativo reais.
node-test:
	node --test tests/changelog_check.test.mjs tests/macos_signing.test.mjs

# Instalação explícita, separada dos testes e do build Cargo.
frontend-deps:
	cd $(FRONTEND_DIR) && npm ci --ignore-scripts --no-fund --no-audit

# Os testes e a checagem de tipos exigem as dependências de frontend-deps.
frontend-test:
	cd $(FRONTEND_DIR) && node popover.test.mjs && node mac-dashboard.test.mjs && node shortcut-recorder.test.mjs

frontend-typecheck:
	cd $(FRONTEND_DIR) && node node_modules/typescript/bin/tsc --noEmit

# Exige sessão gráfica; um SKIP de outro sistema não pode virar aprovação.
macos-native-test:
	@test "$$(uname -s)" = Darwin || { echo "A prova nativa exige macOS." >&2; exit 1; }
	cargo test --locked --test macos_status_items -- --run-native
	cargo test --locked --test macos_webview -- --run-native

changelog-check:
	./scripts/check-changelog-immutable.sh

lint:
	cargo clippy --all-targets --locked -- -D warnings

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

machete:
	cargo machete

smoke:
	@echo "Running live API smoke tests (requires creds in shell env)..."
	cargo test --test live -- --ignored --nocapture
