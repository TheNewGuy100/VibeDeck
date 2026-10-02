# Parallel Agent Workflow

Este arquivo define como agentes paralelos devem registrar e coordenar trabalho no VibeDeck.

## Objetivo

Evitar edicoes conflitantes e deixar claro:

- Qual agente esta trabalhando.
- Qual tarefa ele assumiu.
- Quais arquivos pretende ler ou editar.
- Quais arquivos estao reservados.
- O que foi realmente alterado.
- Quais testes foram executados.
- Quais bloqueios ou pendencias ficaram.

## Arquivo de Registro

O coordenador deve manter um registro ativo em `docs/agent-worklog.md` quando houver duas ou mais tarefas paralelas.

Cada entrada deve seguir este formato:

```md
## Agent: <nome> [Workload: <ID único>]

- Workload ID: `<ID único>`
- Task: <descricao curta>
- Status: planned | in_progress | blocked | completed
- Started: <YYYY-MM-DD HH:mm>
- Finished: <YYYY-MM-DD HH:mm ou ->
- Depends on: <agentes ou none>
- Planned files:
  - `path/to/file`
- Reserved files:
  - `path/to/file`
- Changed files:
  - `path/to/file`
- Tests:
  - `<command>`: pass | fail | not-run
- Runtime validation: <what was validated or not validated>
- Result: <summary>
- Blockers: <none or details>
```

## Regras de Reserva

1. Antes de editar, o agente deve registrar seu arquivo em `Reserved files`.
2. Preferencialmente, cada agente pode reservar no maximo um arquivo para edicao.
3. Dois agentes nao podem reservar o mesmo arquivo simultaneamente.
4. Se a tarefa exigir mais de um arquivo, o agente deve dividir o trabalho, pedir ao coordenador uma nova reserva ou trabalhar apenas em um arquivo por vez.
5. Se a tarefa exigir um arquivo ja reservado, o agente deve parar e informar o conflito ao coordenador.
6. Arquivos de documentacao independentes podem ser editados em paralelo.
7. O agente deve atualizar `Changed files` somente com arquivos realmente alterados.
8. O agente nao deve reverter alteracoes de outro agente.
9. O coordenador e responsavel por resolver integracao, conflitos e validacao final.

## Estados

- `planned`: tarefa registrada, ainda nao iniciada.
- `in_progress`: agente possui reserva e esta trabalhando.
- `blocked`: agente encontrou dependencia, conflito ou falha que impede continuidade.
- `completed`: implementacao e validacao terminaram; reservas podem ser liberadas.

## Ordem do Fluxo

```text
coordenador cria tarefa
  -> coordenador gera Workload ID e envia o mesmo ID no prompt do agente
  -> agente reconhece o Workload ID e registra arquivos planejados na entrada correspondente
  -> coordenador verifica conflitos
  -> agente reserva arquivos
  -> agente implementa ou investiga
  -> agente executa testes
  -> agente registra resultado e arquivos alterados
  -> coordenador integra e valida o conjunto
```

## Regras de Comunicacao

- O agente deve declarar se esta apenas investigando ou editando.
- O agente deve informar o comando exato usado para validar.
- Falhas devem ser registradas com causa ambiental ou tecnica, sem serem ocultadas.
- Testes unitarios, testes de bridge e validacao runtime devem ser diferenciados.
- Build passando sozinho nao equivale a validacao funcional ou visual.
- O agente deve repetir o Workload ID em toda atualização de status e usar esse ID para localizar sua entrada no worklog.
- O agente nao deve atualizar a entrada de outro workload; se nao encontrar seu ID, deve informar o coordenador.

## Trabalho Somente de Pesquisa

Agentes que nao editam nao precisam reservar arquivos, mas devem registrar:

- Escopo pesquisado.
- Arquivos consultados.
- Achados com severidade.
- Hipoteses nao confirmadas.
- Recomendacao para o coordenador.

## Finalizacao

Antes de encerrar, o agente deve:

1. Atualizar o status.
2. Listar arquivos alterados.
3. Listar testes executados e resultado.
4. Registrar limitacoes de runtime ou visual.
5. Liberar mentalmente as reservas; o coordenador decide quando o registro pode ser arquivado.
