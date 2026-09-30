# Changelog

Mudanças do aplicativo visual para macOS com Apple Silicon.
O [histórico original](docs/history/CHANGELOG.original.md) foi preservado sem
reescrita. As referências a plataformas antigas pertencem àquele histórico.

## [Unreleased]

### Adicionado

- Opções globais da barra no clique direito do ícone principal: janela, valor,
  cor, só a conta em uso e centralização. Janela, valor e cor escolhidos ali
  valem para todos os provedores e descartam as escolhas individuais.

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
  menus do app ativado ainda estão sendo montados ou levam até 1 s para
  responder, quando um app sem barra de menus própria fica em primeiro plano ou
  o dono da barra fecha, e logo após a concessão da Acessibilidade. Sem uma
  leitura dos menus, a barra fica no meio da tela, sem cobrir os ícones de
  status.
- Centralização sem travar a barra de menus por segundos quando outro app deixa
  de responder.
- Aviso de token renovado que não pôde ser salvo no Claude e no Codex sem
  pedir comando de terminal; a mensagem orienta um novo login no aplicativo
  do fornecedor e continua classificada como login expirado.
- Orientações de login do GitHub CLI e do Kiro CLI exibidas no cartão, em vez
  da frase genérica, sem deixar passar instruções de terminal.
- Falhas de login e de gravação de credenciais renovadas de todos os
  provedores classificadas como login expirado, com a orientação de cada
  fornecedor, em vez de "Couldn't update".
- Mensagens do Model Studio e do Antigravity sem comando de terminal nem
  edição do arquivo de configuração.

### Removido

- Binários públicos de CLI/TUI e ações que abriam o terminal.
- Interfaces e empacotamentos de outros sistemas.
- Publicação automática, integração com gerenciadores de pacotes e atualização
  baseada nos artefatos do projeto de origem.

### Preservado

- Conectores usados pelo painel, contas existentes, preferências e identidade
  do aplicativo.
- Licença original e histórico das versões anteriores.
