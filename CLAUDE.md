# Instruções do projeto

## Produto e escopo

O AI Usage é um aplicativo gráfico para macOS com Apple Silicon. Preserve o
visual atual, a barra nativa e os dados existentes. CLI/TUI, Windows, Linux,
gerenciadores de pacotes e publicação pública foram retirados do escopo.
Não reintroduza fluxos de terminal para o usuário.

Leia [DEVELOPMENT.md](DEVELOPMENT.md) para preparar o ambiente e
[docs/testing.md](docs/testing.md) para validar alterações. A estrutura atual
está em [docs/architecture.md](docs/architecture.md).

## Invariantes

- O frontend é uma camada de apresentação. Coleta, credenciais, nomes canônicos
  e projeção de métricas pertencem ao Rust.
- Preserve os identificadores de conta, a origem `aiub://localhost`, a chave
  `aiub.tray.layout.v1`, o identificador do bundle e o armazenamento existente.
  Não migre arquivos pessoais durante uma reorganização do código.
- Cache usa escrita atômica e isolamento por conta. `outcome::Outcome` e
  `outcome::fallback` centralizam a política após falha. Preserve o erro original
  quando não houver leitura válida para mostrar.
- Use `format::money` e `format::usd`, inclusive para saldos negativos.
  Projete resets e métricas a partir de `core::sections`, sem tabelas duplicadas.
- Texto de serviços e processos externos é dado não confiável. Sanitize no
  ponto de apresentação. Tokens e respostas brutas de autenticação não entram
  no payload, logs, screenshots, snapshots ou mensagens de diagnóstico.
- Trocas de conta preservam locks, transações, identidade capturada e recuperação
  de falha. Não faça esse trabalho na thread da janela. Não aceite caminhos ou
  comandos arbitrários enviados pelo WebView.
- O Keychain Claude mantém o acesso por `security(1)` conforme a implementação
  existente. Não substitua a escrita por outra API sem revisar a identidade e
  as permissões que o macOS associa ao item.
- Links externos passam pela validação do host. O WebView privilegiado só carrega
  os assets locais previstos. Novos comandos precisam de validação tipada.

## Alterar com evidência

Antes de editar, confira `git status`, leia definição e consumidores e preserve
mudanças alheias. Trabalhe na branch da tarefa, sem commits ou publicação
implícitos. Prefira a menor mudança suficiente.

Testes são herméticos: use caminhos temporários e dependências injetadas.
Não leia `$HOME`, Keychain, arquivos de autenticação, configurações, transcrições
ou credenciais reais em testes comuns. Não imprima ambientes ou arquivos que
possam conter segredos. Testes ao vivo exigem autorização específica.

Uma correção lógica precisa de evidência da falha e do comportamento corrigido.
Rode a validação estreita e depois os gates aplicáveis. Não enfraqueça asserções,
ignore falhas ou substitua uma prova nativa por compilação para declarar sucesso.

Para ajustes nativos, confira PID, caminho e hash do aplicativo realmente aberto.
Um binário de teste aprovado não é prova de instalação. Preserve certificado,
identificador e localização ao atualizar um bundle; faça backup antes de
sobrescrever uma cópia local e valide a permissão real após a atualização.

## Build e manutenção

As dependências do frontend são preparadas explicitamente com o lockfile.
O build Cargo usa o Vite local e falha quando assets ou dependências faltam.
Não instale pacotes, aceite `dist` antigo ou gere uma página vazia como fallback.

Mantenha os guias coerentes com os controles existentes. Documente limitações
sem inventar login, configuração ou atualização que a interface ainda não tem.
A revisão linguística fica com o agente principal. Mudanças de autenticação,
IPC e persistência exigem revisão técnica independente.

Novas mudanças entram em `CHANGELOG.md`. Preserve byte a byte o histórico em
`docs/history/CHANGELOG.original.md`, as versões publicadas e as tags. Use o
checker de histórico; não reintroduza exigências de AUR, Scoop ou crates.io.
Preserve `LICENSE` e avisos de terceiros.

Arquivos retirados vão para a lixeira. Antes de sobrescrever ou apagar arquivos
não versionados, faça backup. Limpeza do repositório não inclui contas, sessões,
configurações pessoais ou caches compartilhados do ambiente.
