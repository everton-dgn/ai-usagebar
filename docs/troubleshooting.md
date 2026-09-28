# Solução de problemas

## O painel não abre ou fecha sozinho

Clique no ícone principal ou use o atalho definido nas configurações. Se o
painel precisa permanecer aberto enquanto você usa outro aplicativo, ative a
fixação no cabeçalho. Um clique fora normalmente fecha um painel não fixado.

Se o tamanho ficou inadequado, use a ação de redefinir o tamanho do painel.
Problemas de clique na barra precisam ser verificados na cópia do aplicativo
que está aberta, inclusive quando um build mais novo existe no repositório.

## Um provedor não aparece

Use **Detectar provedores** e confira se o cartão ou item foi ocultado em
**Configurações → Provedores** ou **Barra**. Se **Só a conta em uso** estiver
ativo, as demais contas daquele provedor não aparecem na barra.

A presença de uma sessão válida é independente da visibilidade do cartão.
Consulte [contas e provedores](accounts.md) para os limites de autenticação.

## Os dados estão antigos ou indisponíveis

Confira a conexão, use **Atualizar** e leia o aviso no cartão. O aplicativo pode
preservar a última leitura quando o serviço falha; essa leitura não comprova
consumo em tempo real. Limites temporários do provedor podem adiar a tentativa
seguinte mesmo após um pedido manual.

Uma sessão expirada precisa ser restabelecida no serviço correspondente. A
interface atual não tem login integrado para todos os provedores.

## A centralização não funciona

Use a ação de autorizar a centralização oferecida pelo aplicativo. O guia abre
a página de Acessibilidade do macOS e identifica a cópia em execução. Autorize
essa cópia e conclua somente quando o guia confirmar o acesso.

Uma entrada antiga com nome semelhante pode pertencer a outro executável.
Mover ou reassinar o bundle também pode exigir nova autorização. Conceder o
acesso e confirmar sua identidade são ações feitas pelo usuário no macOS.

## O macOS pede acesso ao Keychain

A integração Claude pode precisar ler uma sessão já armazenada no Keychain.
Confira qual aplicativo está solicitando o acesso. Para builds locais, a
assinatura estável e a localização fixa do bundle ajudam a manter sua
identidade entre atualizações.

Não copie o conteúdo do Keychain nem arquivos de autenticação para um relatório.
Os cuidados técnicos estão no [guia de desenvolvimento](../DEVELOPMENT.md).

## Há duas cópias na barra

Encerre a cópia antiga pelo seu menu e abra somente o bundle desejado pelo
Finder. Ao iniciar, a cópia instalada reconcilia registros antigos criados pelo
aplicativo e guarda backup antes de alterar o caminho. Se o registro apontar
para outro bundle que ainda existe, ele é preservado. Nesse caso, na cópia que
pretende manter, desligue e ligue **Iniciar ao entrar** para escolher o destino.
Não apague configurações ou contas para resolver uma duplicação de processo.
