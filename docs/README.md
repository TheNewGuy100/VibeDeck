# Documentação do VibeDeck

## Onde encontrar e atualizar

| Documento | Responsabilidade |
|---|---|
| [README do projeto](../README.md) | Produto, instalação e comandos de desenvolvimento |
| [AGENTS](../AGENTS.md) | Regras obrigatórias de segurança e desenvolvimento |
| [Arquitetura](architecture.md) | Mapa do código, componentes e contratos de catálogo/fila |
| [Playback](playback.md) | Integração YouTube Music, UMP/SABR e limites atuais |
| [UI](ui.md) | Contratos visuais, acessibilidade e visualizadores |
| [Decisões](decisions.md) | Escolhas duráveis, motivos e condições de revisão |
| [Investigação](investigation.md) | Bloqueio atual, uso e limites do REA, próximo passo autorizado |
| [Validação](validation.md) | Procedimentos e resultados datados com limites de evidência |

## Política de manutenção

- Cada informação tem um dono na tabela acima. Resumos em outros arquivos usam links; não copiar especificações.
- Atualizar o documento responsável junto da mudança. Não gerar relatório por sessão nem transcrição de ferramentas.
- Distinguir **implementado** (código), **validado** (execução datada e escopo), **hipótese** (não demonstrada) e **planejado** (ainda não existe).
- Registrar decisões como contexto → decisão → consequência → condição de revisão. Identificadores D-001 etc. são estáveis.
- Investigação mantém somente estado ativo, bloqueios e próximo passo. Ao encerrar, mover conclusão para validação ou decisão e remover detalhes transitórios.
- Validação registra data, método, resultado e o que não foi demonstrado. Uma alteração posterior não herda sucesso automaticamente.
- Revisar a data apenas quando o conteúdo for conferido; migração documental não é revalidação do app.
- Não documentar dados privados do provedor. Aplicam-se as [regras de segurança](../AGENTS.md).
- Antes de entregar: conferir links relativos, arquivos obrigatórios e `git diff --check`. Ver [checagem local](validation.md).

## Roteiros de leitura

- Iniciar desenvolvimento: AGENTS → arquitetura → documento da área.
- Trabalhar em reprodução: AGENTS → playback → investigação → validação.
- Alterar UI: AGENTS → UI → validação.

Estrutura criada em 2026-10-21 por migração das referências existentes, sem nova validação de runtime.
