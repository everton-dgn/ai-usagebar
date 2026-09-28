# Testes

As verificações usam o código atual e dados fictícios. Nenhum teste comum pode
consultar credenciais, Keychain ou configuração pessoais.

## Rust e contratos

Execute a partir da raiz:

```bash
CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --all-targets --locked --offline --quiet -- --test-threads=2
cargo clippy --all-targets --locked --offline -- -D warnings
cargo fmt --all -- --check
```

Os módulos de provedores validam parsing, cache e projeção. O relatório tem
fixtures de caracterização. `tests/account_worker.rs` cobre o protocolo privado
com contas fictícias e diretórios temporários. `tests/build_support.rs` cobre
a preparação e validação dos assets incorporados.

A saída de um teste ignorado deve continuar identificada como ignorada.
`--offline` exige dependências disponíveis localmente; sua ausência não autoriza
alterar o lockfile para fazer a validação passar.

## Interface

```bash
make frontend-test frontend-typecheck
```

Os testes do frontend verificam o modelo de apresentação e o HTML renderizado.
Eles não substituem interação real com WKWebView, eventos AppKit ou snapshots
do aplicativo instalado.

## Assinatura e documentação

```bash
make node-test
make changelog-check
git diff --check
```

Os contratos de assinatura usam ferramentas simuladas e bundles temporários.
Os testes de changelog cobrem o arquivo histórico, as tags e o limite de versão.
Não alteram o aplicativo pessoal. O checker de histórico verifica as seções
publicadas no documento original arquivado e as futuras seções do changelog
ativo, além da versão do pacote mantido.

A comparação cobre todas as tags por padrão. Divergências anteriores à limpeza
só são aceitas com a proveniência e os hashes conferidos pelo checker, conforme
o [registro do histórico](history/README.md). Não se altera o arquivo original
para fazê-lo passar.

## Prova nativa

Em uma sessão gráfica do macOS:

```bash
CARGO_NET_OFFLINE=true CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 make macos-native-test
```

| Harness | Evidência |
| --- | --- |
| `macos_status_items` | Eventos e geometria de clique dos botões nativos |
| `macos_webview` | Assets compilados em WKWebView, identidade, painel, estilos, IPC e navegação |

Esses harnesses usam `--run-native`. Sem essa opção, a saída informa `SKIP`;
a suíte comum não conta como execução nativa. O WebView de teste usa dados
fictícios e armazenamento não persistente.

Registre separadamente o resultado desses harnesses e o teste manual do bundle.
Para o segundo, confirme PID, caminho e hash da cópia aberta. Observe reabertura,
clique fora, fixação, redimensionamento, troca de provedor, foco, animações,
preferências após reinício e Acessibilidade. Uma imagem pintada dentro de um
processo não comprova a composição final da barra pelo macOS.

## Rede e autenticação reais

Testes ao vivo são opt-in e ficam fora do gate comum. Uma execução autorizada
precisa usar contas próprias para teste, registrar o que foi validado e impedir
que tokens apareçam no relatório. Mocks e protótipos não comprovam uma jornada
real de login ou recuperação.
