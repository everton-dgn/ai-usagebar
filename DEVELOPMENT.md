# Desenvolvimento

Este repositório mantém o aplicativo visual para macOS com Apple Silicon.
Os comandos deste documento são ferramentas de manutenção do código. O uso do
aplicativo está no [guia de primeiro uso](docs/getting-started.md).

## Ambiente

| Ferramenta | Requisito |
| --- | --- |
| Máquina | Mac com Apple Silicon |
| Rust e Cargo | Rust 1.90 ou superior, conforme `Cargo.toml` |
| Node.js | 22.12 ou superior no fluxo de desenvolvimento |
| npm | Executável compatível com `frontend/package-lock.json` |
| Ferramentas Apple | Compilador, SDK e ferramentas de assinatura |

A versão mínima funcional do macOS precisa de validação nativa. O deployment
target do build não comprova sozinho que todas as integrações funcionam em uma
versão anterior do sistema.

Confira os executáveis antes de instalar dependências: um alias global não deve
substituir o gerenciador usado pelo lockfile. Este frontend mantém
`package-lock.json`; não converta gerenciadores como efeito colateral do build.

Prepare as dependências explicitamente:

```bash
make frontend-deps
```

O alvo usa `npm ci --ignore-scripts --no-fund --no-audit`. A compilação Cargo
não instala pacotes. Repita essa preparação quando o lockfile mudar ou as
dependências estiverem ausentes.

## Build e assinatura

```bash
cargo build --release --locked --bin ai-usagebar-tray
```

O executável fica em `target/release/ai-usagebar-tray`. O build compila a interface
em `frontend/` com o Vite local e incorpora seus assets no executável. HTML,
JavaScript e CSS precisam existir como arquivos regulares e não vazios;
dependência ausente, bundle antigo ou página substituta não são alternativas
aceitas. `AI_USAGEBAR_NODE` permite selecionar o Node usado nessa etapa.

Para preparar o aplicativo completo para abrir pelo Finder:

```bash
make bundle
```

O alvo gera `target/release/AI Usage.app` para arm64 e verifica sua assinatura.
Ele não instala nem abre o bundle. Preserve o `CFBundleIdentifier` da cópia
usada localmente (`ai-usagebar-tray`) ao preparar a substituição.

O script de assinatura aceita um executável ou bundle local:

```bash
scripts/sign-macos-tray.sh 'caminho/AI Usage.app'
```

O script mantém o identificador do bundle e verifica a assinatura.
`CODESIGN_IDENTITY` seleciona o certificado; `CODESIGN_IDENTITY=-` é uma opção
explícita de assinatura ad hoc para desenvolvimento. A troca de identidade pode
exigir nova autorização de Acessibilidade.

Mantenha a cópia de uso em uma localização fixa, como `~/Applications/AI Usage.app`.
Antes de substituir um bundle local, faça backup e encerre a cópia aberta.
Instalação, reinício do aplicativo e publicação são ações distintas da
compilação e exigem autorização da tarefa.

O LaunchAgent usa o executável da cópia instalada. Ao iniciar, o aplicativo
reconcilia registros antigos que ele próprio criou, com backup, fora da thread
da janela. Uma opção desligada continua desligada. Cópias em diretórios de
build não substituem a instalação escolhida; arquivos modificados manualmente
ou que apontam para outro bundle existente são preservados.

## Alterar e validar

1. Confira o Git e preserve mudanças locais preexistentes.
2. Leia implementação, consumidores e testes do comportamento alterado.
3. Faça a menor mudança que atende ao pedido. Correções lógicas devem ter uma
   prova da regressão, seguida da execução sobre a correção.
4. Execute as [verificações aplicáveis](docs/testing.md) e registre falhas,
   testes ignorados e limites da evidência.
5. Atualize os guias afetados e o changelog ainda não publicado.

Para a validação completa local:

```bash
CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --all-targets --locked --offline --quiet -- --test-threads=2
cargo clippy --all-targets --locked --offline -- -D warnings
cargo fmt --all -- --check
make frontend-test frontend-typecheck
make node-test changelog-check
git diff --check
```

O modo offline depende do cache local. Falta de pacote deve ser tratada como
preparação explícita do ambiente, sem instalar dependências durante um teste.
Serialize builds Cargo no mesmo checkout. Desativar o incremental e os símbolos
de teste reduz o espaço ocupado pelos artefatos locais.

## Dados e autenticação

Testes automáticos usam diretórios temporários, dependências injetadas e dados
fictícios. Não podem ler configuração, credenciais, Keychain, transcrições ou
histórico reais. Testes de rede real ficam fora do gate comum e exigem uma
execução autorizada.

Preserve a lógica transacional de ativação de contas, os locks, o isolamento
por conta e a renovação de tokens. Nunca copie a implementação de autenticação
para o frontend ou transforme uma falha de recuperação em uma nova tentativa
silenciosa.

Não registre tokens, chaves, códigos de acesso, URLs OAuth completas ou respostas
brutas de autenticação em logs, snapshots, mensagens de interface ou relatórios.
Os testes do worker de contas usam o próprio executável com um ambiente isolado,
sem abrir a janela ou consultar a conta pessoal.

## Prova do aplicativo instalado

Um teste aprovado em `target/debug` não comprova o comportamento da cópia aberta.
Para validar um ajuste nativo, registre PID, caminho e SHA-256 do executável
real, além da sequência de interação observada. Confira o acesso real à
Acessibilidade no processo, não apenas a presença de uma entrada nos Ajustes.

A [prova nativa](docs/testing.md#prova-nativa) cobre os harnesses disponíveis.
Abertura e reabertura do painel, troca de provedor, fixação, redimensionamento,
foco, animações e preferências após reinício também precisam de observação no
bundle que será utilizado.

## Histórico e entrega

O [changelog ativo](CHANGELOG.md) recebe as mudanças novas. O
[histórico original](docs/history/CHANGELOG.original.md) preserva o documento
anterior sem tradução ou alteração das versões publicadas. A verificação de
histórico compara as seções com as tags existentes.

Commit, push, tags e publicação seguem a autorização específica da tarefa.
Os comandos de build e teste não devem executar essas ações implicitamente.
A licença e os avisos originais permanecem preservados.

Consulte também a [arquitetura](docs/architecture.md), os
[contratos de teste](docs/testing.md) e as [invariantes](CLAUDE.md).
