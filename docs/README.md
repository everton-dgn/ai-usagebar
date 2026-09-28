# Documentação

O produto atual é o aplicativo visual AI Usage para macOS com Apple Silicon.
Os guias de uso descrevem recursos presentes na interface. Protótipos e itens
planejados não são apresentados como recursos disponíveis.

## Usar o aplicativo

- [Primeiro uso](getting-started.md): abrir o bundle, reconhecer a barra e ativar o início automático.
- [Configuração visual](configuration.md): painel, provedores, barra, aparência e alertas.
- [Contas e provedores](accounts.md): detecção, sessões existentes e troca de conta.
- [Solução de problemas](troubleshooting.md): dados indisponíveis, janela e permissões.

## Desenvolver e manter

- [Desenvolvimento](../DEVELOPMENT.md): ambiente, build, assinatura e fluxo de alteração.
- [Arquitetura](architecture.md): frontend, host nativo, núcleo e persistência.
- [Testes](testing.md): comandos, isolamento e prova nativa.
- [Integrações dos provedores](vendor-endpoints.md): localização das fontes e contratos.
- [Instruções do repositório](../CLAUDE.md): invariantes para alterações de código.

## Histórico e planejamento

O [changelog ativo](../CHANGELOG.md) registra mudanças desta versão do produto.
O [changelog original](history/CHANGELOG.original.md) preserva o conteúdo anterior,
inclusive plataformas retiradas e referências ao projeto de origem.
O [registro de preservação](history/README.md) explica sua origem, a checagem
por tags e a divergência histórica conhecida.

O [plano de simplificação](../plans/desktop-app-simplification/PLAN.md) e seu
[checkpoint](../plans/desktop-app-simplification/CHECKPOINT.md) são registros de
trabalho. Use o código e os guias acima para consultar o que está implementado.
