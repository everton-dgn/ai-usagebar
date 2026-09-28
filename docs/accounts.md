# Contas e provedores

O AI Usage acompanha dados das fontes configuradas e de sessões que já existem
no computador. A detecção, a apresentação de consumo e a ativação de uma conta
são operações diferentes.

## O que a interface faz hoje

| Ação | Resultado |
| --- | --- |
| Detectar provedores | Procura fontes existentes e habilita os provedores reconhecidos |
| Atualizar | Consulta novamente os dados das entradas monitoradas |
| Personalizar ou ocultar | Muda a apresentação do cartão ou da barra |
| Trocar conta Claude ou Codex | Ativa uma conta já cadastrada e disponível para troca |

A interface ainda não oferece uma jornada completa de cadastro e login para
todos os provedores nem um formulário geral para chaves de API. Remover os
comandos antigos não cria esses recursos. As configurações existentes continuam
sendo usadas; contas novas que dependam desses recursos permanecem uma limitação
até sua implementação gráfica.

O Nous Research depende de uma credencial salva pela versão anterior do
AI Usage. A consulta e a renovação dessa sessão continuam disponíveis, mas
conectar uma conta Nous nova ainda exige uma jornada de login que não existe
na interface atual.

## Troca de contas

No painel, selecione uma conta Claude ou Codex disponível para ativação.
A ação afeta a sessão usada pela integração correspondente, portanto é diferente
de apenas selecionar um cartão para consultar seu consumo.

A troca acontece fora da thread da janela. O aplicativo espera o resultado,
recarrega a identidade e atualiza os dados para evitar atribuir a leitura de uma
conta a outra. Se houver erro, aguarde o resultado exibido antes de repetir a
operação. Um erro de recuperação precisa de investigação, não de repetição
contínua.

O filtro da barra **Só a conta em uso** altera quais itens aparecem. Ele
não cadastra nem ativa contas.

## Tipos de fonte

Os conectores podem aproveitar sessões de aplicativos, arquivos de autenticação
já existentes, Keychain ou configurações de chave. Manter esses conectores é
necessário para o monitoramento, inclusive quando a origem pertence a uma
ferramenta externa com interface de terminal. O AI Usage não oferece uma CLI
pública para operar esses dados.

Assinaturas e APIs de cobrança são fontes distintas. Por exemplo, Claude e
Anthropic API têm autenticação e métricas próprias. Um plano pago não garante
que toda API de administração esteja acessível com a mesma conta.

## Dados ausentes ou vencidos

O cabeçalho usa a identidade da própria conexão consultada. A descoberta é
automática quando a fonte autenticada fornece um e-mail válido:

| Provedor | Fonte de identidade |
| --- | --- |
| Claude e Codex | Metadados da sessão local correspondente à conta |
| Kimi | Perfil `/coding/v1/me`, com o mesmo token usado na consulta de consumo |
| Antigravity | `userStatus.email` da sessão local; o fallback remoto não herda esse endereço |
| GitHub Copilot | Perfil GitHub; lista de e-mails somente quando o token já tem permissão, escolhendo o principal verificado |
| Cursor | Perfil `/api/auth/me`, com o mesmo cookie da consulta de consumo |
| Kiro | Campo opcional `userInfo.email` da resposta de uso |
| Nous Research | Campo opcional `user.email` da resposta de conta |

Quando o campo não está disponível, o painel mostra **E-mail indisponível nesta
conexão**. Isso também se aplica aos demais provedores. Uma chave de API pode
identificar uma organização, equipe ou projeto sem expor o e-mail de uma pessoa.
O aplicativo não preenche esse dado com o endereço de outra conta.

Na consulta real de 28/09/2026, os endpoints de consumo de Z.AI/GLM e MiniMax
responderam com sucesso e sem e-mail. A pesquisa de suas APIs públicas não
identificou um endpoint de perfil utilizável com essas mesmas chaves. Portanto,
a leitura do consumo funciona, mas a identificação dessas contas permanece
indisponível pela conexão atual.

A consulta de perfil é auxiliar: falhas ou ausência de e-mail não removem as
métricas de consumo. Os endereços obtidos por API ficam fora dos caches de
consumo em disco e são omitidos nos logs de depuração. A interface recebe apenas
a identificação validada, vinculada à entrada correspondente. Após reiniciar,
algumas fontes precisam da próxima resposta de uso para preencher o e-mail.

1. Confira no aplicativo ou na página oficial do provedor se sua conta está ativa.
2. Volte ao AI Usage e use **Detectar provedores**, se a fonte ainda não aparecer.
3. Use **Atualizar** e confira a mensagem do cartão.

A detecção não substitui a autenticação. Se o método exigido ainda não tiver um
fluxo gráfico disponível, o aplicativo não consegue concluir a conexão por sua
interface atual.

Os tokens e as chaves não devem aparecer em capturas de tela, logs de suporte ou
fixtures. Para investigar um problema, registre a mensagem exibida e o nome do
provedor, sem copiar credenciais.
