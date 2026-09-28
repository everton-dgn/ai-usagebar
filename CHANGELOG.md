# Changelog

Mudanças do aplicativo visual para macOS com Apple Silicon.
O [histórico original](docs/history/CHANGELOG.original.md) foi preservado sem
reescrita. As referências a plataformas antigas pertencem àquele histórico.

## [Unreleased]

### Alterado

- Produto concentrado no aplicativo visual do macOS, com a interface existente.
- Frontend organizado em `frontend/`, separado dos arquivos antigos do Windows.
- README e guias separados por uso, contas, configuração, manutenção e testes.
- Capturas da interface atual no README, com contas e consumo demonstrativos.

### Corrigido

- Identidade automática da conta transportada pelos conectores que fornecem
  e-mail autenticado, sem misturar contas nem gravar endereços no cache de uso.
- Indicação explícita quando a conexão não disponibiliza e-mail.
- Cache do Kimi vinculado à credencial e à região, para impedir que uma troca
  de conta reutilize o consumo da conexão anterior.
- Reconciliação segura do início automático após mudança da localização antiga
  para o bundle instalado, preservando a opção desligada.
- Apresentação da tecla Command e tradução do detalhe de ritmo em português.
- Verificação de todas as tags com proveniência das divergências históricas,
  mantendo o changelog original intacto.

### Removido

- Binários públicos de CLI/TUI e ações que abriam o terminal.
- Interfaces e empacotamentos de outros sistemas.
- Publicação automática, integração com gerenciadores de pacotes e atualização
  baseada nos artefatos do projeto de origem.

### Preservado

- Conectores usados pelo painel, contas existentes, preferências e identidade
  do aplicativo.
- Licença original e histórico das versões anteriores.
