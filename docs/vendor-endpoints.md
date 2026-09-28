# Integrações dos provedores

Esta referência é para manutenção do código. O uso do aplicativo e os limites da
conexão de contas estão em [contas e provedores](accounts.md).

O catálogo, a configuração e os conectores definem quais fontes estão habilitadas.
O frontend apresenta as entradas produzidas pelo núcleo; não deve limitar a
lista a uma tabela antiga de plataformas suportadas.

## Onde conferir cada integração

| Provedor | Fonte |
| --- | --- |
| Claude | [`anthropic`](../src/anthropic/) e [`claude_desktop`](../src/claude_desktop/) |
| Anthropic API | [`anthropic_api`](../src/anthropic_api/) |
| Codex | [`openai`](../src/openai/) |
| GitHub Copilot | [`copilot`](../src/copilot/) |
| Z.AI / GLM | [`zai`](../src/zai/) |
| OpenRouter | [`openrouter`](../src/openrouter/) |
| DeepSeek | [`deepseek`](../src/deepseek/) |
| Kimi | [`kimi`](../src/kimi/) |
| Kilo | [`kilo`](../src/kilo/) |
| Novita | [`novita`](../src/novita/) |
| Moonshot | [`moonshot`](../src/moonshot/) |
| Grok API | [`grok`](../src/grok/) |
| SuperGrok | [`supergrok`](../src/supergrok/) |
| Grok Bot | [`grokbot`](../src/grokbot/) |
| Antigravity | [`antigravity`](../src/antigravity/) |
| Cursor | [`cursor`](../src/cursor/) |
| MiniMax | [`minimax`](../src/minimax/) |
| Kiro | [`kiro`](../src/kiro/) |
| Nous | [`nous`](../src/nous/) |
| OpenCode Go | [`opencode_go`](../src/opencode_go/) |
| Command Code | [`commandcode`](../src/commandcode/) |
| Ollama | [`ollama`](../src/ollama/) |
| OrcaRouter | [`orcarouter`](../src/orcarouter/) |
| Model Studio | [`modelstudio`](../src/modelstudio/) |
| Personalizados | [`custom`](../src/custom/) |

Os módulos de busca e de tipos são a fonte para endpoints, autenticação e formato
aceito. Ao alterar uma integração, confira a implementação e os testes do
provedor, a configuração, o catálogo e a projeção em `core::sections`.
Não deduza disponibilidade atual de uma API apenas a partir de uma URL no código.

## Contratos a preservar

- Uma resposta HTTP bem-sucedida pode conter uma falha no corpo. Z.AI e MiniMax
  validam o envelope antes de tratar a resposta como consumo.
- As porcentagens recebidas podem representar saldo restante. A projeção precisa
  manter a convenção de consumo usada pelo painel.
- Janelas e resets precisam vir dos campos que identificam seu significado;
  não atribua nomes pela posição de uma lista.
- Falha na consulta deve preservar a política de cache e o diagnóstico aplicável.
  Uma leitura antiga não se torna nova por ter sido reapresentada.
- Identidade de conta e região precisam acompanhar o isolamento dos dados.
  Não reutilize um resultado de outra conta nem deduza e-mail a partir de uma chave.

Algumas integrações dependem de endpoints sem contrato público estável.
Fixtures protegem os formatos conhecidos, mas não comprovam disponibilidade ao
vivo. Testes de rede e autenticação reais são opt-in, conforme o
[guia de testes](testing.md#rede-e-autenticação-reais).
