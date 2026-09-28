# Integração nativa do macOS

Esta pasta reúne recursos nativos do AI Usage. O host Rust fica em
[`src/tray`](../src/tray/) e a interface do painel em
[`frontend`](../frontend/).

- [Primeiro uso](../docs/getting-started.md).
- [Configuração visual](../docs/configuration.md).
- [Desenvolvimento e assinatura](../DEVELOPMENT.md).
- [Testes nativos](../docs/testing.md).

O produto é o bundle gráfico para macOS com Apple Silicon. O antigo aplicativo
Swift e os comandos de instalação dos binários de terminal foram retirados.
A identidade existente do bundle e a origem do WebView permanecem preservadas.
