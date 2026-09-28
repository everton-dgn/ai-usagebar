# Arquitetura

O aplicativo tem um frontend React dentro de WKWebView e um host Rust integrado
à barra de menus do macOS. O executável público do produto é o aplicativo de
barra; não há uma interface de linha de comando para operá-lo.

## Responsabilidades

| Área | Código | Responsabilidade |
| --- | --- | --- |
| Interface | `frontend/src/` | Telas, componentes, preferências de apresentação e tradução |
| Host macOS | `src/tray/host_macos.rs` | Ciclo da janela, eventos, coleta e conexão com o WebView |
| Barra | `src/tray/status_items.rs`, `menu_bar.rs`, `menu_space.rs` | Itens por provedor, desenho, cliques e posicionamento |
| Fronteira do WebView | `src/tray/ipc.rs`, `assets.rs`, `browse.rs` | Comandos tipados, assets locais e links externos |
| Entradas e coleta | `src/core/entries.rs`, `refresh.rs` | Identidade das entradas e atualização dos dados |
| Projeção | `src/core/sections.rs`, `src/report.rs` | Métricas, resets, links e relatório visual |
| Contas | `src/core/accounts.rs`, `src/tray/account_worker.rs` | Operações de conta e processo auxiliar privado |
| Integrações | `src/anthropic/`, `src/openai/` e módulos dos provedores | Autenticação suportada, busca e interpretação dos dados |
| Persistência | `src/config.rs`, `cache.rs`, `outcome.rs` | Configuração, cache, escrita atômica e política de fallback |
| Build | `build.rs`, `src/build_support.rs` | Compilação e incorporação da interface |

## Fluxo dos dados

O núcleo enumera as fontes habilitadas, coleta seus dados e monta o relatório.
O host acrescenta os fatos locais necessários à apresentação e envia o payload
ao frontend. A interface envia comandos tipados ao host para alterar preferências
ou executar uma ação. Rede e troca de contas não devem bloquear a thread nativa.

Nomes canônicos de provedores, dinheiro, limites e metadados de reset pertencem
ao Rust. O frontend não mantém uma segunda interpretação dos dados brutos da API.
Texto externo precisa de tratamento no ponto de apresentação.

## Conta e identidade

A ativação Claude/Codex usa um processo auxiliar privado do próprio executável.
O protocolo transporta uma operação limitada, com provedor e rótulo validados;
a interface não envia caminhos arbitrários ou comandos de shell. O worker
retorna um resultado tipado, sem tokens ou diagnóstico bruto.

O e-mail obtido de uma resposta autenticada passa por `identity::AccountEmail`
e acompanha `Outcome`, `ReadyTab` e a entrada do relatório interno até o painel.
Seu `Debug` oculta o endereço, que fica separado do cache de métricas em disco.
Claude e Codex também usam marcadores locais: o host omite a identificação se
ela mudar durante a consulta. Nunca se usa a identidade de outro provedor para
preencher um campo ausente. As fontes disponíveis estão no
[guia de contas](accounts.md).

## Persistência e compatibilidade

`config::default_path()` fornece a localização canônica do macOS.
`config::resolved_path()` escolhe o arquivo efetivo e mantém o arquivo legado
`~/.config/ai-usagebar/config.toml` quando aplicável. Uma reorganização do código
não deve mover arquivos pessoais de forma silenciosa.

As preferências visuais usam a chave `aiub.tray.layout.v1`. A origem
`aiub://localhost`, o armazenamento do WebView e a identidade
`ai-usagebar-tray` do bundle local fazem parte da compatibilidade do aplicativo.
O LaunchAgent mantém o rótulo `com.akitaonrails.ai-usagebar-tray`; esse nome
herdado não implica dependência de publicação do projeto original. Dimensões
da janela e configurações da barra também são persistidas.

Cache e backoff ficam isolados por provedor e, quando aplicável, por conta.
Ao trocar uma conta, a leitura em andamento e a identidade capturada precisam
ser reconciliadas antes de apresentar um resultado novo.

## Limites

Os conectores podem reutilizar sessões de aplicativos externos. Sua presença
não significa que o AI Usage ofereça uma tela de login para cada serviço.
O [guia de contas](accounts.md) descreve as capacidades visíveis atuais.
Testes de modelo e SSR não provam a composição da barra nativa nem a preservação
de permissões após instalar outro bundle.
