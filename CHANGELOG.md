# Changelog

Mudanças do aplicativo visual para macOS com Apple Silicon.
O [histórico original](docs/history/CHANGELOG.original.md) foi preservado sem
reescrita. As referências a plataformas antigas pertencem àquele histórico.

## [Unreleased]

### Alterado

- README com captura da barra de menus em uso, preferências visuais e lista
  completa dos 24 provedores, sem identificação pessoal nas imagens.
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
- Recuperação do consumo após falha de rede durante uma rotação de token do
  Kimi realizada pelo aplicativo, preservando o isolamento entre contas.
- Leitura do cache de consumo do Copilot e do Cursor sem aguardar a consulta
  opcional de perfil; o e-mail validado é mantido apenas em memória.
- Verificação da credencial Desktop antes de dar preferência a ela sobre uma
  conta configurada com o mesmo nome.
- Reconciliação segura do início automático após mudança da localização antiga
  para o bundle instalado, preservando a opção desligada.
- Proteção contra registrar cópias temporárias no início automático e backups
  do LaunchAgent em uma pasta persistente do aplicativo.
- Substituição do bundle com a lixeira nativa quando o comando `trash` não está
  disponível, preservando uma cópia do bundle anterior.
- Apresentação da tecla Command e tradução do detalhe de ritmo em português.
- Verificação de todas as tags com proveniência das divergências históricas,
  mantendo o changelog original intacto.
- Barra de provedores centralizada após a troca de aplicativo, mesmo quando os
  menus do app ativado ainda estão sendo montados ou quando um app sem barra de
  menus própria fica em primeiro plano, e logo após a concessão da
  Acessibilidade.
- Centralização sem travar a barra de menus por segundos quando outro app deixa
  de responder.

### Removido

- Binários públicos de CLI/TUI e ações que abriam o terminal.
- Interfaces e empacotamentos de outros sistemas.
- Publicação automática, integração com gerenciadores de pacotes e atualização
  baseada nos artefatos do projeto de origem.

### Preservado

- Conectores usados pelo painel, contas existentes, preferências e identidade
  do aplicativo.
- Licença original e histórico das versões anteriores.
