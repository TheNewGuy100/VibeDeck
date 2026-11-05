# VibeDeck — regras obrigatórias

## Leitura por tarefa

- Começar pelo [índice da documentação](docs/README.md) e pela [arquitetura](docs/architecture.md).
- Reprodução/YouTube Music: ler [playback](docs/playback.md), [investigação ativa](docs/investigation.md) e [validação](docs/validation.md).
- UI: ler e manter em contexto os [contratos visuais e de acessibilidade](docs/ui.md).
- Decisões arquiteturais: [registro de decisões](docs/decisions.md).
- Atualizar o documento responsável junto da mudança, sem duplicar histórico. `YOUTUBE-MUSIC-API.md` é apenas um redirecionamento.

## Produto

Cliente pessoal leve: Tauri 2, Rust, Vue 3/TypeScript e Vite, não React.
Baixo consumo de RAM/CPU é requisito. Preservar separação entre protocolo,
decoder/output e coordenação do player. Destino arquitetural: SABR/UMP e seek
nativos, sem brokers no playback; ver decisões D-001 e D-002.

## Segurança e evidência

- Innertube/UMP/SABR são interfaces internas instáveis, não APIs públicas. Considerar termos/restrições do serviço.
- Não registrar/exportar/fixturear cookies, tokens, assinaturas, API keys, visitorData, IDs de conta/faixa/playlist, IPs, URLs completas, queries privadas ou corpos crus. Contexto e bytes de mídia só transitam em memória no pipeline; não são documentação/logs. O catálogo funcional não deve virar fonte de dumps.
- Logs/diagnósticos: allowlist de etapa, categoria fixa, correlação por ativação, status, contagens, tempos e metadados numéricos limitados. Erros crus do provedor/reqwest podem conter URLs; não propagá-los para logs.
- Ferramentas de observação (formato de request, trace do navegador, logpoints, cache do player) só emitem projeção sanitizada e seguem a [D-006](docs/decisions.md): sem valores, sem URLs completas, cache local do script público fora do projeto.
- Diferenciar leitura de código, fixture sintética, observação do browser e validação nativa audível. HTTP 200/build/parser bem-sucedido não prova som, faixa completa ou seek.
- Requests/testes autenticados exigem autorização explícita e escopo contado; não reutilizar autorização consumida. Não iniciar playback, seek, search ou mutações automaticamente. Search pode afetar histórico da conta.
- Não alterar playlists existentes. Qualquer futuro teste de escrita exige autorização atual e playlist temporária única, reversão e verificação da exclusão. Autorização histórica não é permissão contínua.
- Não contornar CSP/X-Frame-Options; a página completa do YouTube Music não pode ser usada como iframe genérico.
- Preservar mudanças alheias. Não restaurar arquivos excluídos nem apagar trabalho não relacionado para “limpar” o repo. Não fazer commit automaticamente.

## GitNexus — obrigatório ao alterar código

- Usar query por conceito para explorar fluxos e context para callers/callees, em vez de começar por grep de código desconhecido.
- **Antes de editar qualquer função/classe/método:** executar impact upstream do símbolo, comunicar callers/processos/risco; avisar antes de seguir se HIGH/CRITICAL.
- Índice avisou stale: `npx gitnexus analyze` antes de usá-lo.
- Rename usa ferramenta semântica, nunca find/replace. Antes de commit, executar detect_changes e conferir escopo/fluxos esperados.
- Recursos: `gitnexus://repo/VibeDeck/context`, `/clusters`, `/processes`, `/process/{name}`. Skills locais em `.claude/skills/gitnexus/`; contagens do índice mudam e não são contrato.

## Entrega

Seguir os procedimentos de [validação](docs/validation.md). Registrar o que foi
executado agora sem herdar resultados históricos. Mudanças somente documentais
exigem checagem de links/arquivos e diff, não requests nem playback.

<!-- gitnexus:start -->
# GitNexus — Code Intelligence

This project is indexed by GitNexus as **VibeDeck** (4446 symbols, 9915 relationships, 300 execution flows). Use the GitNexus MCP tools to understand code, assess impact, and navigate safely.

> If any GitNexus tool warns the index is stale, run `npx gitnexus analyze` in terminal first.

## Always Do

- **MUST run impact analysis before editing any symbol.** Before modifying a function, class, or method, run `gitnexus_impact({target: "symbolName", direction: "upstream"})` and report the blast radius (direct callers, affected processes, risk level) to the user.
- **MUST run `gitnexus_detect_changes()` before committing** to verify your changes only affect expected symbols and execution flows.
- **MUST warn the user** if impact analysis returns HIGH or CRITICAL risk before proceeding with edits.
- When exploring unfamiliar code, use `gitnexus_query({query: "concept"})` to find execution flows instead of grepping. It returns process-grouped results ranked by relevance.
- When you need full context on a specific symbol — callers, callees, which execution flows it participates in — use `gitnexus_context({name: "symbolName"})`.

## Never Do

- NEVER edit a function, class, or method without first running `gitnexus_impact` on it.
- NEVER ignore HIGH or CRITICAL risk warnings from impact analysis.
- NEVER rename symbols with find-and-replace — use `gitnexus_rename` which understands the call graph.
- NEVER commit changes without running `gitnexus_detect_changes()` to check affected scope.

## Resources

| Resource | Use for |
|----------|---------|
| `gitnexus://repo/VibeDeck/context` | Codebase overview, check index freshness |
| `gitnexus://repo/VibeDeck/clusters` | All functional areas |
| `gitnexus://repo/VibeDeck/processes` | All execution flows |
| `gitnexus://repo/VibeDeck/process/{name}` | Step-by-step execution trace |

## CLI

| Task | Read this skill file |
|------|---------------------|
| Understand architecture / "How does X work?" | `.claude/skills/gitnexus/gitnexus-exploring/SKILL.md` |
| Blast radius / "What breaks if I change X?" | `.claude/skills/gitnexus/gitnexus-impact-analysis/SKILL.md` |
| Trace bugs / "Why is X failing?" | `.claude/skills/gitnexus/gitnexus-debugging/SKILL.md` |
| Rename / extract / split / refactor | `.claude/skills/gitnexus/gitnexus-refactoring/SKILL.md` |
| Tools, resources, schema reference | `.claude/skills/gitnexus/gitnexus-guide/SKILL.md` |
| Index, status, clean, wiki CLI commands | `.claude/skills/gitnexus/gitnexus-cli/SKILL.md` |

<!-- gitnexus:end -->
