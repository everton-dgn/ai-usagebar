# Primeiro uso

## Requisitos e instalação

Use um Mac com Apple Silicon e um bundle local **AI Usage.app** preparado para
essa máquina. A versão mínima funcional do macOS ainda não foi validada em uma
matriz de versões. O projeto não oferece um instalador público ou atualização
automática neste fluxo.

1. No Finder, coloque o bundle em uma pasta fixa de Aplicativos. A pasta
   Aplicativos da sua conta é a localização usada no desenvolvimento local.
2. Abra **AI Usage.app**. O ícone aparece na barra de menus; a janela não depende
   de uma sessão de terminal.
3. Clique no ícone principal para abrir o painel. Use o botão de configurações
   para ajustar o aplicativo.

Se você recebeu apenas o código-fonte, a preparação do bundle é uma atividade
de desenvolvimento, descrita no [guia técnico](../DEVELOPMENT.md).

## Dados dos provedores

Use **Detectar provedores** para procurar fontes já disponíveis no computador.
A ação não cria uma conta nem autentica uma sessão. Quando faltarem dados,
consulte [contas e provedores](accounts.md).

O painel permite atualizar os dados e alternar entre lista e abas. Cada item
individual na barra abre o painel do respectivo provedor. O menu de clique
direito reúne suas opções.

## Configuração inicial

Em **Configurações → Geral**, ajuste a frequência de atualização e o atalho
para abrir o painel. Ative **Iniciar ao entrar** para abrir o aplicativo ao
iniciar a sessão do macOS.

Ao abrir o bundle instalado, o aplicativo atualiza o caminho de uma instalação
legada reconhecida, com backup, se o início automático já estava ligado.
Uma opção desligada permanece desligada. Se o item aponta para outro bundle
que ainda existe, ele é preservado: desligue e ligue a opção na cópia que deseja
manter para escolhê-la explicitamente.

Em **Preferências**, escolha idioma e aparência. Em **Barra**, ajuste quais
provedores aparecem e como o consumo é apresentado. Veja a
[configuração visual](configuration.md) para os demais controles.

Ao ativar a centralização, siga a orientação de Acessibilidade exibida pelo
aplicativo. A permissão deve corresponder à cópia que você abriu. O macOS pode
pedir sua confirmação de identidade; o aplicativo só considera o acesso
concedido após verificá-lo.

## Encerrar ou substituir a cópia

Use **Sair** no menu do aplicativo. Antes de substituir o bundle por outro
build local, encerre a cópia aberta e mantenha um backup. Preserve a localização,
o identificador e a assinatura para manter a continuidade das permissões.
A substituição do bundle não exige apagar contas, preferências ou caches.
Se houver outra cópia instalada, confira **Iniciar ao entrar** conforme
descrito acima.
