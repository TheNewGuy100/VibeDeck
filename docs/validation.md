# Validação

Consolidado em 2026-10-21. Migração documental não reexecuta testes.
[Índice](README.md).

## Procedimento offline de código

```powershell
npm test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --offline --manifest-path src-tauri/Cargo.toml --lib
cargo check --offline --manifest-path src-tauri/Cargo.toml --bin vibedeck
```

Conferir exit do processo real; filtros de stderr/pipelines PowerShell podem mascará-lo.
Nenhum desses comandos prova áudio, continuidade ou seek.

Executor offline independente (não faz parte do build/test do app):

```powershell
cargo test --offline --locked --manifest-path tools/decipher-eval/Cargo.toml
cargo fmt --manifest-path tools/decipher-eval/Cargo.toml -- --check
cargo tree --offline --locked --manifest-path tools/decipher-eval/Cargo.toml -e features
```

Primeira preparação requer dependências do lockfile no cache Cargo; não contornar
offline silenciosamente. Não habilitar features de allocator customizado/host loader.

## Runtime manual

- Tauri: desktop/mobile-width, temas, drawer, loading, overflow e foco. `body { min-width: 960px; }` ainda limita mobile; catálogo visual com screenshots pendente.
- App empacotado/Task Manager: metadata e agrupamento Windows, não apenas Vite. Não renomear processos WebView2.
- Visualizadores: active/paused, histórico até 10 s, intensidade min/max, tooltip tempo/frequência/clamp, troca/limpeza e isolamento do background. FPS sustentado em GPU real não medido.
- Testes autenticados exigem autorização atual e contada. Healthcheck completo faz busca/leitura e pode afetar histórico. Não executar automaticamente.

### Ferramenta de sessão isolada (Windows debug)

1. Fechar normalmente outra instância de desenvolvimento antes de iniciar; não matar processos por rotina.
2. Executar `npm run session:debug`. A entrada é `session-debug.html`, não a tela normal.
3. Somente com autorização atual: **Abrir login** uma vez; completar autenticação manualmente. Não acionar busca/faixas na página do provedor.
4. **Capturar e salvar** uma vez, confirmando a substituição do snapshot. O login fica aberto; não há polling/retry automático. Falha de auth não grava o snapshot.
5. **Verificar snapshot local** é uma leitura local separada; `authEligible` não significa aceitação pelo serviço.
6. **Restaurar e comparar** uma vez: jar privada deve estar vazia, restaura e relê sem sair de `about:blank`. Exigir `comparisonPerformed=true`, `matched=true`, contagem não zero e zero missing/changed/unexpected. Não interpretar `matched=false` em etapas sem comparação como falha de roundtrip.
7. Fechar login e ferramenta normalmente. Não iniciar bootstrap, player ou healthcheck neste roteiro.

O relatório compara nome/domínio/path, valor, host-only, Secure, HttpOnly,
SameSite e expiração sem exportar esses dados. Expirados no instante final são
excluídos dos dois lados. Jar viva pode mudar durante login; este teste não demonstra
sincronização contínua nem aceitação de cookies após reiniciar. Incognito, baseline,
fechamento e integração IPC/COM ainda precisam de execução real.

Teste offline da página: `node --test scripts/session-debug.test.mjs`.

## Ferramentas de observação ([D-006](decisions.md))

Todas emitem só projeção sanitizada: nomes, números de campo, classes de tamanho, presença,
igualdade. Nenhuma lê credenciais nem imprime valor, URL completa ou fonte.

```powershell
npm run lab:test
cargo test --offline --locked --manifest-path tools/decipher-eval/Cargo.toml
cargo test --offline --locked --manifest-path src-tauri/Cargo.toml --lib
```

**Trace de referência do navegador** (`scripts/sabr-trace`). Não envia nada a servidor algum.

1. Abrir `music.youtube.com` no navegador integrado, sem login nem ação extra.
2. Injetar no console `shape-core.cjs` e depois `hook.js`.
3. Iniciar **uma faixa nova**: o `/player` precisa disparar depois do hook, senão a relação do `n` fica `unknown`. O play é ação do usuário e cada play tem escopo próprio.
4. Ler `__sabrTraceSummary()` e desinstalar com `__sabrTraceUninstall()`. O resumo traz, por POST SABR, nomes de parâmetros, campos do protobuf (incluindo o campo 2 do `StreamerContext`), `nRelation` (`equal`/`different`) e classes da resposta, e do `/player` as chaves do corpo e se `serviceIntegrityDimensions.poToken` foi enviado.

**Dry-run nativo.** `debug_youtube_sabr_transport_once` com `dryRun: true` e orçamento
`{auxiliaryGet:1,playerPost:1,sabrPost:0}` faz o GET e o player, monta a request exata e
projeta seu formato sem POST SABR. Compara-se o `requestShape` com o resumo do navegador.
Consome a tentativa do processo e exige autorização contada.

**Logpoints por CDP** (`scripts/cdp-lab`): `targets` lista alvos por origem; `run --plan <arquivo>
--origin <origin>` instala logpoints que nunca pausam. Exige um Chrome iniciado com porta de
depuração em loopback. Os `needle` dos planos são específicos de cada revisão do player.

**Cache local do player.** `player_lab ingest|counts|list|purge`
(`cargo run --manifest-path tools/decipher-eval/Cargo.toml --example player_lab -- <comando>`)
guarda o script público em `%LOCALAPPDATA%\VibeDeck\player-lab`, por 24 h e até 3 revisões, e
recusa um caminho dentro do projeto. O `ingest` normaliza `\r\n` para `\n`, porque a área de
transferência do Windows converte as quebras e isso mudaria a identidade da revisão.

**Oráculo do `n`** (`ejs_oracle`, laboratório, não faz parte do app). Executa o solver do yt-dlp
(`yt-dlp-ejs` 0.8.0) num QuickJS novo, sem binding de host (sem rede, arquivos ou processos), com
limites bem acima dos do executor do app (1 GiB de heap, pilha de 256 MiB numa thread dedicada,
prazo de 5 min). Compara a saída do solver, para `n` sintéticos, com a sequência candidata.
Obtenção do solver: baixar o wheel `yt_dlp_ejs-0.8.0-py3-none-any.whl` de `files.pythonhosted.org`
(53.443 bytes, SHA-256 `79300e5fca7f937a1eeede11f0456862c1b41107ce1d726871e0207424f4bdb4`), conferir o
hash contra o PyPI, extrair fora do projeto e conferir que nenhuma entrada escapa da pasta. Os dois
JS usados são `core.min.js` e `lib.min.js`. Uso: `ejs_oracle <pasta do solver> --selftest` (player
sintético, prova o encanamento) e `ejs_oracle <pasta do solver> [--id <id>]` sobre o player em
cache. Autoteste em 2026-10-22: 6 de 6 entradas sintéticas bateram, 90 ms. Executado em 2026-10-22
contra o script real em cache (2.626.523 bytes após normalizar): o solver terminou sem erro em
59 s; **0 de 6** entradas sintéticas bateram com a sequência candidata, e as saídas têm caracteres
ausentes da entrada e 5 caracteres a menos. A sequência candidata foi refutada. A comparação com o que o navegador envia, por valor, foi feita depois (resultado abaixo).

**Validação do `n` por valor, sem que valor algum chegue ao chat.** `scripts/sabr-trace/validate-n.js`,
colado no console de uma página `music.youtube.com`, guarda só em memória os `n` recebidos do
`/player` e os `n` enviados nos POSTs SABR. `scripts/sabr-trace/solve-clipboard.ps1` espera a lista
de `n` recebidos na área de transferência, resolve com `ejs_oracle --stdin` (solver em
`%LOCALAPPDATA%\VibeDeck\ejs-solver`) contra o player em cache e devolve os pares ao clipboard.
`__nv.compare(pares)` responde na página só com booleanos e comprimentos. O play e o player em
cache precisam vir do mesmo navegador (a variante do script varia). Rodar o solver dentro da
página foi descartado: o `lib.min.js` comprimido tem 45,7 KB, `eval` em iframe é bloqueado pela
CSP (sem isolamento) e o parse de 2,6 MB travaria o player. Testes de 2026-10-22: 2 do script da
página e 2 do modo `--stdin`; a metade local passou de ponta a ponta com entradas sintéticas
(cerca de 60 s). Exercitado com um play real em 2026-10-22, no Chrome do mesmo script em cache (2.626.523 caracteres): `compare` devolveu `all_received_map_to_sent`, com 2 de 2 pares e os 2 `n` enviados explicados pelo solver, 19 caracteres de entrada e 14 de saída; solver em 54 s. Limites: dois pares, uma revisão e uma variante do script, `n` recebidos de faixas tocadas naquele momento. Na primeira tentativa do usuário o
`compare` devolveu `not_pairs`, que não dizia o que foi colado; passou a nomear o erro sem eco
(`got_the_received_list_not_pairs`, `solver_reported_error`, `empty_pairs`, `pasted_text_not_json`),
o oráculo avisa por contagem quando descarta pares, e o `solve-clipboard.ps1` explica cada falha
(lista com entradas fora do padrão, solver sem pares, saída inesperada) sem imprimir valores.

Offline em 2026-10-22, sem requests e sem execução contra o provedor: 11 testes do trace e do
CDP, 3 do `session-debug`, 80 do executor e 129 do app (default features) passaram, com
`cargo fmt --check` limpo nas duas crates. O hook foi testado com `XMLHttpRequest` falso e o
CLI do cache com um diretório temporário. Não foram exercitados o hook num play real, o dry-run
com bootstrap real nem o cliente CDP contra um Chrome.

Observação do token no player real (`scripts/sabr-trace/pot-observe.js`, 2026-10-22): hook passivo,
só para o navegador integrado. Cole no Console de uma aba de music.youtube.com (recarregue antes com
F5 para tirar hooks antigos), toque duas faixas diferentes por cerca de 30 s cada e rode
`copy(JSON.stringify(__po.summary()))`. O resumo traz índices de token (1, 2, ...), classes de
tamanho, contagens, segundos e booleanos: onde o token viaja (campo 2 do `StreamerContext`, parâmetro
`pot`, corpo do `/player` com gzip aberto), se é por vídeo ou por sessão, se muda dentro de um
vídeo e as chamadas de atestação (só o nome e a classe de status). Nenhum valor sai da página.
Testado offline com 3 testes (`npm run lab:test`, 17 no total); ainda não exercitado num play real.

## Últimos resultados conhecidos

M0, medição de sessões sem token, 2026-11-05 (player em cache `ef321720`, 2.662.178 bytes; solver validado
por entrada sintética: 17 caracteres viraram 13): 3 sessões com orçamento 1/1/6 e seek a 50%, cada uma num
processo novo. **As 3 receberam `401_403` na rodada 0** (`sabr_probe_http_access_denied`); 1 de 6
requests foi enviada em cada sessão e nenhuma chegou a estado de proteção, cookie ou mídia. Placar acumulado
sem token, com `n` resolvido e formato do navegador: **1 aceita e 5 recusadas** na primeira request. A
única aceita foi a primeira tentativa de todas (22/10). Rodadas seguintes e seek continuam sem observação.
Com isso o caminho sem token se comporta como recusado de forma consistente; a causa não está isolada
(token, `n`, vínculo por IP ou reputação são hipóteses). Pelo sinal de D-007 (401/403 na primeira
request), o agendador cairia no relay em todas as faixas.

Token no player real, 2026-10-22 (`pot-observe.js`, 176 s, 3 faixas, só índices e classes): 51 POSTs SABR,
**todos com o campo 2 do `StreamerContext`**. Três tokens SABR distintos, **um por faixa**, cada um com 85
bytes, estável dentro da faixa (36 e 14 POSTs sem troca) e já presente na primeira request de cada
uma. Nenhum `pot` em URL. Os dois `/player` observados (corpo gzip aberto) levam `poToken` no
corpo, **o mesmo nas duas chamadas e com 10 bytes**, diferente de todos os tokens SABR: parece um
token de outro tipo (de sessão), não o token do vídeo. O token da faixa apareceu no mesmo instante do
`/player` dela (143,8 s), e a faixa seguinte, pré-carregada, já tinha o seu (145,8 s). Nenhuma
chamada de atestação (`Waa`) em 176 s: o gerador é reaproveitado e cada token novo sai localmente, sem
rede. Não observado: a quem o token é ligado (provável ID do vídeo, não demonstrado), renovação em
faixas longas, seek.

Sessão SABR de 6 requests, 2026-10-22 (`debug_youtube_sabr_session_once`, orçamento 1/1/6, a última
request seria um seek a 50%): `n` resolvido pelo solver, formato do navegador, sem PO token, igual
à tentativa de uma request abaixo. **A primeira request recebeu `401_403`**
(`sabr_probe_http_access_denied`) e a sessão parou; nenhuma das outras 5 foi enviada. A forma da
URL (nomes e classes) é idêntica à da tentativa que deu 2xx. Portanto o 2xx anterior **não é
reproduzível por enquanto**: com as mesmas entradas, uma execução aceitou e outra recusou. Hipóteses
não testadas: enforcement do token por experimento (`fexp` muda a cada `/player`), tempo ou
reputação do IP depois das tentativas anteriores, diferença no corpo da primeira request (os
temporizadores do campo 1 da sessão eram pequenos, não zero; corrigido depois, e o relatório agora
traz a forma da request de cada rodada). Isso enfraquece a leitura "o token não é exigido" da
entrada seguinte: ela vale como uma observação, não como regra.

Repetição da sessão com o código corrigido, 2026-10-22 (orçamento 1/1/6, seek a 50%, processo novo):
de novo `401_403` na primeira request, 1 de 6 enviada. A forma da request da rodada 0 (relatório novo)
é igual à da tentativa que deu 2xx: mesmos nomes e classes na URL, `cpn`, `cver`, `rn` e `alr`, sem
cabeçalhos, corpo com os campos 1, 5 e 19, `ClientInfo` 1/16/17/18/19, temporizadores zerados. Então a
diferença **não está no formato** que medimos. Placar com as mesmas entradas: 1 aceita, 2 recusadas.
Leitura mais simples, ainda sem prova: a aceitação sem token é intermitente (experimento `fexp`,
risco do IP ou da sessão), o que torna o PO token necessário para uma reprodução confiável.

Tentativa contada de transporte SABR, 2026-10-22 (orçamento 1/1/1, uma por processo): `n` transformado
pelo solver yt-dlp-ejs dentro do QuickJS, contra o player em cache; URL e corpo no formato observado
no navegador (`cpn` e `cver` na URL, `alr=yes`, sem campo 16, `ClientInfo` 1/16/17/18/19, nenhum
cabeçalho próprio); **sem PO token** (campo 2 ausente). Resultado, só projeção segura: `2xx`,
`content-type` UMP, 25 partes, 13 de mídia (331 904 bytes), 1 de inicialização, uma parte de
proteção com `status` 1, sem redirect, reload nem erro, 3 partes desconhecidas; `rawNSent` falso,
`nSolved` verdadeiro. Em formato, o `n` enviado caiu da classe 16–255 para 1–15 (como no navegador).
Leitura: com `n` correto e formato espelhado, **a primeira request SABR de áudio do WEB_REMIX foi
aceita sem PO token**. Isso mostra que o `n` cru e/ou o formato causavam o 401/403 daquela
vez; as duas sessões seguintes (acima) mostram que o token continua possivelmente necessário. Não prova que o token nunca seja exigido: foi uma request só, no tempo
zero, e o significado de `status` 1 da parte de proteção não está confirmado (o 2/3 visto antes
era a recusa). Os valores de `hl`, `os_name` e `os_version` do `ClientInfo` são suposições do
experimento (`en`, `Windows`, `10.0`), não dados capturados.

Trace de referência do navegador, 2026-10-22 (um play do usuário no navegador integrado; só
formato, sem valores): a primeira instalação do hook registrou zero requests porque lia o corpo só
em `init.body` e o player usa `fetch(new Request(...))`; corrigido e coberto por teste, e a segunda
instalação capturou 12 POSTs SABR (outros 12 descartados pelo limite) e 4 trocas do `/player`.
Todos os POSTs SABR retornaram 2xx com `ump`. Resposta com cabeçalhos `cache-control`,
`client-protocol`, `content-type` e `expires`. Resultados:
`n` da `serverAbrStreamingUrl` com 16 ou mais caracteres; `n` do POST com no máximo 15, e diferente
do último `n` recebido, portanto o cliente transforma o `n` (a comparação por valor sozinha seria
ambígua com várias trocas do `/player`; a mudança de classe de tamanho vale para todas).
Campo 2 do `StreamerContext` presente em todas as requests, classe 16–255. URL com os mesmos
parâmetros recebidos mais `cpn`, `cver`, `rn` e `alr`. Corpo de 256–4095 bytes com os campos 1,
(2), (3), 5 e 19, sem o 16; `ClientInfo` com os campos 1, 16, 17, 18 e 19. A resposta do `/player`
não traz `serviceIntegrityDimensions`; o corpo do pedido `/player` chegou comprimido e não foi
lido. Limites: um play só, uma faixa por vez, uma revisão do player, sem o valor de nenhum campo.
Hook removido ao final e `fetch`/`XMLHttpRequest` restaurados.

Diagnóstico offline do `n` em 2026-10-22 (sem requests nativos, sem ledger, sem player/SABR/áudio):
`tools/decipher-eval/examples/n_counts.rs` lê um script de player da entrada padrão e
imprime só contagens. Um script de 2.635.854 bytes copiado de um Chrome (revisão não
comprovadamente igual à baixada pelo Rust, que tinha 2.624.029 bytes) retornou urlEntries=1,
nInputs=0, `.get("n")` ×1, `.set("n",…)` ×0 (literal, em qualquer formato de sink), 14 literais
`"n"`, nenhum binding inicializado com `"n"` e nenhum literal como argumento 1 de chamada.
Um navegador integrado, em leitura léxica, deu 1/0/14 para os mesmos três valores e 2.624.029
caracteres. Isso não prova se o `n` é ou não necessário na URL SABR real. Passaram 76 testes
do executor e 5 do `transport_probe`; o guard de URL agora emite códigos separados para `s`,
`n` e ambos, coberto por teste sintético.

Gate B em 2026-10-22, após recompilar e reiniciar o app (janela isolada, sem a feature
`native-playback-trial`): uma invocação com orçamento exato 1 GET auxiliar + 1 player + 1 SABR
retornou `sabr_probe_url_transform_required_n`. Pelo caminho de código, GET/player e bootstrap
concluíram e a preparação da URL bloqueou antes do POST SABR; a URL tem `n` e não tem `s`.
Zero transporte SABR, mídia e áudio; sem captura independente de tráfego. O app anterior
(PID de 15:54) foi encerrado por PID e o novo iniciado às 17:35. Tentativa por processo
consumida; não demonstra o formato do `n` nem a causa do `UNPLAYABLE` histórico.

Gate B com `n` cru em 2026-10-22 (flag de debug `allowRawN`, padrão desligado; app recompilado
às 17:46 e iniciado em seguida, PID novo): uma invocação, orçamento exato 1 GET + 1 player +
1 SABR. O POST SABR foi enviado com o `n` exatamente como recebido (sem `s`) e retornou
`sabr_probe_http_status_rejected`: status HTTP sem sucesso. A categoria do status (401/403,
429, 4xx, 5xx) existia em `successful_status`, mas o probe a descartou, então não se sabe qual
foi. Zero mídia e áudio; sem captura independente de tráfego. Tentativa por processo consumida.
Não prova que o `n` cru seja a causa: a request também não leva PO token (poToken de zero
bytes no bootstrap), cookies/Authorization nem o estado completo do cliente. Cobertura sintética
nova: 6 testes do `transport_probe` e 120 do app (default features).

Categoria de status do SABR, offline em 2026-10-22: o probe agora projeta o status HTTP sem
sucesso em códigos fixos (`sabr_probe_http_{redirect,access_denied,rate_limited,client_status,
server_status,unexpected_status}`), sem número nem corpo. Coberto por teste sintético; 121 testes
do app passam (default features).

Gate B com `n` cru e categoria de status, em 2026-10-22: app recompilado às 17:51 e iniciado em
seguida (PID novo); uma invocação autorizada, orçamento exato 1 GET + 1 player + 1 SABR, `n`
enviado como recebido e sem `s`. Retorno: `sabr_probe_http_access_denied`, ou seja, 401 ou 403
no POST SABR (o número exato não é projetado). Zero mídia e áudio; sem captura independente de
tráfego. Tentativa por processo consumida. Uma primeira invocação desta rodada foi bloqueada por
permissão antes de executar, sem requisição. Não distingue a causa: o `n` cru, a ausência de PO
token (poToken de zero bytes), a ausência de credenciais ou o estado incompleto do cliente
podem, cada um, produzir acesso negado; nenhuma dessas causas foi isolada.

Leitura estática do PO token em 2026-10-22, sem requests nativos e sem ledger: no player servido
ao navegador integrado (2.624.029 caracteres), léxica e sem execução, o token é produzido no
cliente por atestação (`Waa/Create`, snapshot do BotGuard, `Waa/GenerateIT`). Aparece no corpo
do `/player`, no campo 2 do `StreamerContext` (só quando presente) e como `pot` em pedidos DASH
por GET. O código do cliente não o exige para montar a request SABR; quem exige é o servidor, e
isso não foi observado. O pedido nativo não envia token em nenhum desses pontos. Não foi lido
como o valor é obtido em `Dn()` nem como é decodificado antes do campo 2.

Continuação estática em 2026-10-22: helper local temporário leu fontes já carregadas
via Debugger.getScriptSource no CDP dedicado e parseou com Babel, sem Runtime.evaluate,
downloads remotos ou persistência das fontes. Três scripts player: 2626522, 103486 e
120595 bytes UTF-8; nenhum com sourceMapURL reportado. Principal difere em tamanho
do adquirido pelo Rust (2624029 bytes). Inventário do principal: 15183 funções e sete
chamadas com argumento literal n; apenas uma .get('n'), em sincronização query→path,
não em transformação. Uma referência sintática direta a esse helper; sem resolução
lexical comprovada ou execução dinâmica. Fonte/digest nativos antigos indisponíveis;
identidade não validada. Consulta summary à evidência retida confirmou registro,
mas não expôs motivos dos mapas; sem novo fetch, play ou consumo do ledger.

Observação web/REA em 2026-10-22 (separada do trial nativo): um play manual
autorizado na aba integrada produziu playing 1 ms após play e pausa automática
em 30014 ms; mídia confirmada pausada. Não foi obtida cadeia de chamadas nem
exportado cpuprofile. Essa autorização de um play está consumida; não prova áudio
nativo, faixa completa ou seek. No Chrome dedicado, REA observou 26 scripts;
análise estática parseou 13 sem falhas (~2,16 milhões de nós AST), e inspeção do
video encontrou 20 listeners com cleanup confirmado. Não demonstra transformação n.
Aquisição inicial e uma repetição autorizada dos três mapas declarados retornaram
fetch_failed para todos, sem mapas/fontes originais recuperados. Motivo específico
ainda não determinado: a projeção ocultou limitation; recuperação posterior da
evidência completa excedeu orçamento MCP. Não interpretar como 404/403 confirmado.
Não houve play iniciado pelo REA nem consumo adicional do ledger nativo nessas
operações. Método, riscos e próximo passo em [investigação](investigation.md#rea-como-bancada-de-análise).

Continuação integrada em 2026-10-22: tentativas 3–5 consumiram mais três GETs
Music/scripts, sem player/SABR/áudio. Tentativa 3: recognitionCounts=[1,0,0,0,0,0]
com reconhecimento limitado a inicializadores. Após visitar Function AST, tentativa
4: recognitionCounts=[12409,1976,46,3,497,2], urlEntries=1, nInputs=0,
trial_n_call_not_unique. Tentativa 5 repetiu essa estrutura e nShapeCounts com nove
zeros: nenhuma chamada com argumento literal n na entrada selecionada. Não prova
ausência global de transformação nem equivalência semântica da entrada ao provider.
Novo guard para extração não pronta evita POST player inútil. Total persistente:
5 Music/5 scripts/2 player, zero SABR/bytes/áudio. Nesta execução passaram **71 testes
offline/locked do executor**, incluindo entrada atribuída e parâmetro adicional;
build nativo abriu nas três execuções. Também passaram **123 testes Rust lib do app
com native-playback-trial**, offline/locked/um job. Fontes reais descartadas; sem
dump/fixture.

Checkpoint integrado em 2026-10-22: tentativas 1 e 2 retornaram player OK,
scriptBytes=2624029, timestampProperties=2 e urlEntries=0. Ambas pararam em
trial_url_transform_failed, com extração trial_url_entry_not_unique. Consumo
acumulado 2 Music/2 scripts/2 player, zero SABR/bytes/áudio. Não prova transformação,
transporte ou som. Correção sintética de aridade >=3 não resolveu a segunda tentativa.
Nesta execução passaram 70 testes offline/locked do executor, com um job.
Build nativo serial abriu após liberar memória; falhas anteriores de alocação não
consumiram requests. Vite ignora src-tauri/target para evitar watcher sobre artefatos.


Checkpoint bootstrap/script em 2026-10-22: **1/1 GET Music + 1/1 script consumidos**
na alocação nova, sem reutilizar/resetar alocação estrutural 2/20. Uma invocação
retornou script_diagnostic_inspected, bootstrapBytes=793317, scriptBytes=2624029,
declarationSelected=true, timestampProperties=2, timestampIntegerLiterals=2,
executionPerformed=false, playerBindingValidated=false, cobertura
declaration_provenance_and_script_ast_only. Fluxo exige HTTP 200/MIME JS/UTF-8,
mesma URL solicitada, GET sem cookies/auth no script; não houve captura independente
de tráfego. Dois literais não significam dois valores distintos nem identificam STS
relevante: comparação/contexto não implementados. HTML/script descartados em memória,
sem fixtures/dumps/valores/revisão/URL exportados. Zero execução JS/POST player/SABR/áudio.

Nesta preparação passaram offline/locked **67 testes experimentais, 17 da fachada,
122 Rust lib com bootstrap-inspection**, além de cargo check do binário sem feature.
Novos testes sintéticos: seleção do caminho exato, duplicatas e ancestrais duplicados,
computed/spread/condição/path externo, inventário timestamp em código morto e strings,
parse inválido/limite, destino canônico fixo e reserva persistente exata sem reabertura.
Cliente CDP passou node --check; readiness repete só probes locais, invoke único.
GitNexus registro handler LOW/zero callers/processos; novos símbolos UNKNOWN.
Warnings dead-code legados persistem. Nenhum teste novo do frontend/decoder.

Atualização 2026-10-22 — alocação ampliada: **2/20 GETs Music consumidos**, ledger
preservado entre restarts. Ordinal 1 teve partial=true por inline de 706.114 bytes.
Após ampliar somente diagnóstico a 1 MiB, **65 testes experimentais, 15 da fachada
e 120 Rust lib com bootstrap-inspection passaram offline/locked**. Teste sintético
de padding ~706 KiB verifica diagnóstico aceito e descoberta elegível ainda rejeitada.
Cliente CDP passou node --check e readiness usa probes locais sem repetir invoke.
Ordinal 2 retornou html_ast_syntax_only, 798.866 bytes HTML, 17 inline/sete externos,
maior inline 700.749 bytes, zero falhas JS/oversized/membros config não suportados,
partial=false e scriptCountLimited=false. Uma propriedade estática jsUrl, literal
canônica em chamada direta config e caminho Music Watch; ambiguous=false,
bindingValidated=false. Fonte descartada, sem exportar valores. Sem download de
script, POST player/SABR, áudio ou captura independente de tráfego. Resultados
anteriores abaixo são históricos, não saldo/autorização atual.

Checkpoint estrutural em 2026-10-22: uma inicialização/restart isolada --no-watch,
uma invocação Rust aceita e um GET autenticado, segundo o fluxo do comando; sem
captura independente de tráfego. Retorno bootstrap_structure_script_limit: pode
significar script >256 KiB ou contagem >128, não identifica qual nem confirma
localização de jsUrl. Fonte descartada, sem dumps/URLs/revisão/credenciais exportados.
Zero GET iframe_api/script, POST player/SABR ou áudio. Primeira conexão CDP falhou
com código local genérico; segunda na mesma instância retornou o código Rust,
demonstrando que a reserva única ainda não havia sido consumida pelo primeiro
cliente. Guard agora consumido, sem reset. Margem humana de dois GETs não utilizada;
restart adicional não autorizado. Fechamento normal antigo não encerrou o processo;
PID app conferido e encerrado pontualmente, sem apagar dados/cache. Instância nova
mantida aberta. Esse resultado não valida HTML/AST completo ou binding.

Preparação: **63 testes experimentais, 13 da fachada de parsers e 118 Rust lib do
app com --features bootstrap-inspection passaram** offline. Fachada não liga QuickJS;
cargo tree conferido. Dois testes novos de contexto cobrem chamada direta e caminho
Music Watch sintéticos, código morto/aninhado, outro receiver, duplicatas/spread/
getter/computed. Dois testes app cobrem orçamento/campos extras/reserva única e
projeção sem valores/binding sempre falso. GitNexus handler LOW/zero callers/processos;
novos símbolos UNKNOWN. Código posterior separou script_source_limit e
script_count_limit, mantendo os limites. Após essa distinção, os **63 + 13 + 118**
testes foram repetidos e passaram offline/locked; cargo check offline/locked do
binário sem feature também passou. Warnings dead-code legados persistem.
Sem mudanças no resolver, decoder ou frontend; estes não foram retestados.

Seleção HTML offline em 2026-10-22: **61 testes da crate isolada passaram**, zero
doc-tests, após cargo fmt. Quatro novos testes cobrem comentários/template/noscript/
SVG/textarea, src inclusive vazio, JSON/module/nomodule, type maiúsculo, entidades
em raw text, duplicatas intra/entre scripts, JS inválido, HTML reparado/atributos
duplicados e limites de fonte/script/contagem. Não testado limite de nós ou pico de
recursos. Dependências html5ever/markup5ever_rcdom 0.38.0 obtidas do registry Cargo,
lock próprio atualizado; testes depois offline/locked. GitNexus sem fluxo/contexto,
impact do símbolo novo UNKNOWN. Sem requests YouTube, leitura de sessão, restart,
script remoto ou áudio. App/IPC/resolver não alterados nem retestados. Não valida
contexto real jsUrl, binding, decipher ou transporte. Mudanças alheias preservadas.

Análise estrutural offline de jsUrl em 2026-10-22: **57 testes da crate isolada
passaram**, zero doc-tests. Três testes novos cobrem decoys em comentários/strings/
regex, escapes de chave, projeção sem valores, shorthand/chamadas/null, computed
literal, getter/método, duplicatas em código morto/aninhado, assignment fora de
cobertura, parse inválido e limite de fonte. GitNexus query/context sem símbolos,
impact da função nova UNKNOWN; nenhum caller/processo identificado. Somente fixtures
sintéticas: sem HTML real, requests, sessão, restart ou áudio. App/IPC/resolver não
alterados nem retestados. Seleção HTML e contexto semântico permanecem pendentes.

Inspeção bootstrap autorizada em 2026-10-22: uma inicialização isolada com --no-watch
e uma invocação de debug_youtube_bootstrap_once, orçamento {auxiliaryGet:1}. Retorno:
bootstrap_probe_inspected/lexical_only, 803757 bytes, quatro marcadores crus de
caminho, zero escapados, sem truncamento. jsUrl: uma ocorrência string e um caminho
candidato distinto. PLAYER_JS_URL/js/STS: zero matches nas formas lexicais suportadas.
Sem fonte/valores/URLs/revisão exportados ou persistidos. Fonte descartada ao retornar.
Isso não localiza semanticamente o campo, não confirma declaração executada, vínculo
ou compatibilidade com player. Pelo código, um GET autenticado concluído; sem captura
independente de tráfego. Zero POST player/iframe_api/script/SABR/áudio. Tentativa e
orçamento encerrados, sem retry. Primeira conexão CDP parou antes do invoke por IPC
indisponível; segunda aguardou prontidão local e fez a única invocação na mesma
instância, sem restart. Instância isolada mantida, sem montar App.vue.

Preparação do diagnóstico: **116 testes Rust lib passaram** com offline e
profile.test.incremental=false. Três testes novos cobrem orçamento único, projeção
sem valores/tipos e falsos positivos em comentário/limites. Primeira compilação
falhou no build script embed-resource ao consultar rustc; repetição passou, sem
apagar cache. Build script legado expôs variável sensível na saída de falha: não
reproduzir esse valor; sanitização desse build script é pendência separada. Launcher
temporário filtra linhas sensíveis. GitNexus handler LOW/zero callers/processos;
comando novo não encontrado (UNKNOWN). Fmt check, sintaxe do cliente CDP, links e
diff check passaram antes desta atualização final. Frontend/crate experimental não
alterados/retestados. Não comprova parser HTML/JS estrutural.

Reconferência pública em 2026-10-22: Player.ts, Session.ts e Innertube.ts do commit
78375cbf808544d91b0d5df99529a20f4a5a18eb lidos via raw GitHub. Confirmados por leitura:
origem iframe_api sem override, aquisição player_es6 (getter usa player_ias) e envio
do timestamp extraído em getBasicInfo/getInfo. Não confirma declaração no HTML Music,
vínculo atestado pela resposta ou compatibilidade atual WEB_REMIX. Nenhuma fonte do
YouTube adquirida/executada, request autenticado, restart ou áudio nesta rodada.
Mudança somente documental; suítes de código não reexecutadas.

Descoberta experimental offline em 2026-10-22: **54 testes da crate independente
passaram**, zero doc-tests, após cargo fmt. Quatro novos testes cobrem declaração
literal/escapes/offset Unicode, duplicatas iguais ambíguas, contexto condicional ou
aninhado/redefinição/mutação/getter/spread/chamada e ausência/parse/limites/caminhos
inválidos. Somente fontes sintéticas; chave de fixture hipotética não observada.
GitNexus query/context/impact sem símbolos (UNKNOWN). Nenhuma edição Rust no app,
request, sessão, fonte real, áudio ou restart; launcher ativo preservado. Não há
evidência nova da origem do script real ou de vínculo config→POST player. Suítes do
app não repetidas. Reconhecedor é experimento, não correção integrada de assets.js.

Inspeção real em 2026-10-22: uma invocação autorizada de
debug_youtube_player_script_once via CDP/IPC, após inicialização autorizada da página
isolada session-debug.html (sem montar App.vue). Orçamento máximo 1 GET auxiliar,
1 player e 1 script, sem retry/SABR/áudio. Retorno allowlistado:
playabilityStatus OK, result player_script_identity_missing, scriptBytes 0.
Pelo fluxo Rust, GET/player concluíram; assets.js ausente ou não-string impediu GET
do script. Não distingue ausência de tipo inválido, não demonstra origem alternativa
nem valida transformação. Sem captura independente do tráfego. Tentativa encerrada,
sem reaproveitar saldo ou repetir. Sessão protegida lida somente pelo fetch nativo;
sem recaptura/dumps. Fonte real não adquirida nem executada. Instância isolada mantida.

Preparação IPC desta rodada: **113 testes Rust lib passaram** com offline e
profile.test.incremental=false; teste novo cobre orçamento exato, campo extra e
reserva única. Registro do handler teve impacto GitNexus LOW/zero callers/processos
no grafo; símbolos novos não encontrados (UNKNOWN). Crate experimental/decoder/UI
não alterados nem retestados. Sintaxe do cliente CDP temporário passou. Cliente
faz uma única invocação e só imprime projeção sanitizada; nenhuma tentativa repetida.

Coordenador de inspeção em 2026-10-22: cargo test --offline --manifest-path
src-tauri/Cargo.toml --lib --quiet --config profile.test.incremental=false passou
primeiro com 109 testes (incluindo os quatro de aquisição) e depois com **112**
após três novos testes de orçamento exato/campos extras, falha de identidade
consumindo tentativa e oito reservas concorrentes com um único vencedor.
Nenhum cache apagado/config persistente alterado; desabilitar incremental no comando
superou a falha anterior, mas não diagnostica sua causa. Fixtures só sintéticas,
sem invocar fetch/IPC, requests, sessão, restart ou áudio. Módulo sem callers no app;
GitNexus query/context/impact sem símbolos (UNKNOWN). Warnings dead-code persistem.
Não prova política HTTP contra servidor, vínculo ao player real ou transformação.

Aquisição de script em 2026-10-22: cargo check --offline --tests passou, com warnings
dead-code. Adicionados quatro testes sintéticos de vínculo ao payload exato versus
clone, identidade ausente, caminhos não canônicos/destinos externos e limites de
fonte/UTF-8/MIME. **Não executados com sucesso**: cargo test --offline --lib falhou
na compilação final por ausência do formato rlib de windows, webview2_com e tao.
Não apagar cache nem herdar os 105 testes da rodada anterior como resultado atual.
Fetch assíncrono não invocado; sem teste HTTP, sessão real, requests, restart ou áudio.
GitNexus query/context sem fluxo e impact do binding novo UNKNOWN. Alterações
restritas ao módulo novo, registro em sabr/mod.rs e documentação; cadeia incompleta.

Preparação HIC em 2026-10-22: **50 testes offline da crate JS** e **105 testes Rust
lib do app passaram**. Dois novos testes JS exercitam argumentos tipados com aspas,
controle/NUL/Unicode, limites, loop interrompido, saída objeto/Promise e erro fixo.
Cinco novos testes nativos cobrem preservação da query, n codificado/duplicado,
conflitos assinados, envelopes rejeitados, destino/encoding inválidos, ausência de
desafio sem callback e limites/erros de resultado. Callbacks são sintéticos; não
provam decipher nem vínculo ao bootstrap. Primeiro comando nativo excedeu 120 s;
repetição dos cinco testes concluiu e depois suíte lib completa passou.
Cargo check offline do binário passou; warnings dead-code persistem, inclusive no
helper novo ainda sem callers. GitNexus query/context/impact não localizaram símbolos
(UNKNOWN); alterações restritas ao executor experimental, módulo puro novo e registro
em sabr/mod.rs. Sem mudanças em auth/IPC/engine/UI, downloads, requests, leitura de
sessão, restart ou áudio. Frontend não alterado/retestado. Cadeia audível ainda incompleta;
checkpoints e trabalho restante em investigação/playback.
Fmt check de ambas as crates, arquivos/links relativos e git diff --check passaram;
os 50 testes experimentais foram repetidos após formatação com sucesso.

Gate de parâmetros em 2026-10-22: **48 testes offline passaram**, zero doc-tests,
após formatação (quatro testes novos). Fixtures cobrem defaults primitivos, rejeição
da IIFE/throw, funções/arrows/métodos aninhados, funções não selecionadas,
destructuring, rest e defaults não literais. Gate percorre sintaxe, sem executar
fontes ou certificar corpos. Impact GitNexus do símbolo novo não encontrado
(UNKNOWN). Sem mudanças de app/autenticação, requests, sessão, áudio ou restart;
suítes do app não repetidas.

Fixture funcional em 2026-10-22: **44 testes offline passaram**, zero doc-tests,
após cargo fmt (dois testes novos em structure.rs). Original sintético com stub local
de URL retornou a transformação esperada com argumentos explícitos e defaults;
variante com default IIFE/throw passou estrutura, retornou Execution ao omitir o
argumento e funcionou ao fornecê-lo. Somente fontes/argumentos sintéticos fixos,
runtimes novos; não houve reconstrução nem execução de código remoto. GitNexus
impact do teste novo não encontrado (UNKNOWN). Sem alteração de autenticação/app,
requests, sessão, playback ou restart; suítes do app não repetidas.

Revisão estática adicional em 2026-10-22: conferidos gate estrutural e caminho de
avaliação. Documentadas condições de inicialização da raiz, insuficiência funcional
dos positivos h.URL e necessidade de separar efeitos por fase/ownership. Não houve
geração, execução de fixture nova ou modificação de código; 42 testes anteriores
não constituem evidência de reconstrução funcional. Query/context GitNexus não
localizaram o fluxo/símbolo. Sem requests, sessão, playback ou restart.

Gate estrutural em 2026-10-22: **42 testes offline passaram**, zero doc-tests,
na crate experimental (seis novos). Fixtures cobrem const diretas, strictness
herdada tolerada sem geração, var condicional, blocos/IIFE, var/let na entrada,
statements externos com mutação/escape/reatribuição e inicializadores não selecionados
com efeitos/destructuring. Corpos/defaults continuam sem classificação de efeitos;
passar estrutura não prova elegibilidade executável. GitNexus impact do símbolo novo
não encontrado (UNKNOWN). Sem mudanças de app, requests, sessão, playback ou restart;
suítes do app não repetidas.
Após formatação, os 42 testes foram repetidos; fmt check, arquivos/links relativos
e git diff --check passaram.

Análise estática em 2026-10-22 de extraction.rs, resolution.rs e reconstruction.rs:
identificadas lacunas de kind/local de inicialização, controle de fluxo, strictness,
usos externos e validação da entrada; contrato e próximo gate em playback.
Não foram executadas fixtures novas nem alterado código. GitNexus query sem
resultados e context de plan_reconstruction não encontrado; cobertura limitada.
Sem requests, sessão, execução JavaScript, playback ou restart. Suítes de código
não repetidas nesta revisão exclusivamente documental.

Plano de reconstrução em 2026-10-22: cargo test --offline --locked da crate
experimental passou com **36 testes** (cinco novos), zero doc-tests. Novos casos
sintéticos cobrem nomes iguais em escopos distintos, ordem original diferente da
ordem de dependências, ancestralidade de bloco/função, Unicode e não inclusão de
throw do contêiner. Plano não gera/executa código nem prova inicialização, efeitos
ou compatibilidade real. GitNexus query/context/impact sem símbolos (UNKNOWN);
nenhum caller/processo do app identificado. Sem requests, áudio ou restart.
Após formatação, 36 testes repetidos com sucesso; fmt check, links relativos e
git diff --check passaram. Suítes do app não reexecutadas nesta mudança isolada.

Resolver lexical validado em 2026-10-22: crate experimental usa oxc_semantic 0.95.0 pinado;
dependências obtidas via cargo fetch, sem requests ao YouTube. Testes sintéticos
cobrem resolução transitiva/deduplicação, sombreamento, binding em contêiner aninhado,
captura externa rejeitada, referências ausentes, initializers com efeitos/aliases,
escritas externas, ciclos, ambientes dinâmicos, orçamento e referências em defaults
e propriedades computadas. Não chama o executor ou valida semântica real do player.
GitNexus query/context/impact não encontraram os símbolos (UNKNOWN). Mudanças
isoladas na crate e documentos; sem alteração de IPC/playback, restart ou áudio.
Executados agora: cargo test --offline --locked com **31 testes passando** (11
executor, 9 reconhecimento, 11 resolução), zero doc-tests; cargo fmt -- --check,
checagem de arquivos/links relativos e git diff --check passaram. Este último
emitiu somente avisos de conversão LF/CRLF. Suítes do app não foram reexecutadas.

Reconhecimento AST em 2026-10-21: `cargo test --offline --locked --manifest-path
tools/decipher-eval/Cargo.toml` passou com **20 testes** (11 executor + 9 novos de
reconhecimento), zero doc-tests. Oxc 0.95.0 e dependências foram obtidos por cargo
fetch do registry; testes depois rodaram offline. Fixtures são exclusivamente
sintéticas: unicidade/ambiguidade, parse inválido, função aninhada sem executar
contêiner, comentários/strings/regex, formas de parâmetros/função, statements diretos,
limites de fonte/trecho e Unicode. Candidate contém slice original, sem Debug/Serialize;
nenhuma resolução de dependências ou chamada ao executor implementada.
GitNexus query sem resultados e context/impact sem símbolos da crate experimental
(UNKNOWN); lib.rs só exporta o módulo novo, sem callers do app. App, IPC/probe,
sessão, UI e decoder não foram alterados nesta rodada. Nenhum download de script
YouTube, request autenticado, áudio ou restart. Suítes do app não reexecutadas.
Não prova semântica da função, compatibilidade real, isolamento ou limites absolutos
de recursos do parser. Fonte limitada não herda limites de memória/interrupt do QuickJS.
Após formatação, os 20 testes foram repetidos com sucesso; fmt check, arquivos/links
relativos e git diff --check passaram.

Executor durável em 2026-10-21: `cargo test --offline --manifest-path
tools/decipher-eval/Cargo.toml` passou com **11 testes**, zero doc-tests. Cargo fmt
executado e árvore de features conferida: rquickjs 0.14.0 somente std, sem
rust-alloc/allocator/loader/dyn-load/futures da engine. Lockfile próprio gerado
offline. Testes sintéticos cobrem fonte/saída limitadas, bytes Unicode, tipos sem
coerção, rejeição de Promise/vazio/prefixo de erro, host ausente, isolamento de
globais, exceção/sintaxe sanitizadas, alocação/recursão e interrupção de loop com e
sem try/catch. O wrapper rejeita resultado se deadline expirou; não se infere desse
teste sozinho que todo erro da engine seja incapturável. Allocation/recursion
produziram Execution, não Interrupted, sem classificar a exceção interna.
Crate em tools/decipher-eval não é dependência do app e não tem HTTP/IPC/sessão.
GitNexus query sem resultados; context de prepare_url e impact do novo
evaluate_synthetic não encontrados (UNKNOWN), sem callers do app por construção.
Nenhum teste autenticado, download de player, áudio, restart ou commit; suítes do
app não reexecutadas pois seu código/dependências não mudaram. Não prova decipher,
compatibilidade do player, teto de RSS total ou isolamento contra vulnerabilidade nativa.

Spike de executor em 2026-10-21, fora do repositório:
`%LOCALAPPDATA%/Temp/opencode/vibedeck-js-eval`, cargo run com rquickjs=0.14.0,
default-features=false/features std, compilado neste Windows. Seis casos sintéticos
passaram: resultado determinístico (1 ms), cinco nomes de host undefined (0 ms),
loop infinito interrompido (100 ms), crescimento ArrayBuffer retornou erro (1 ms),
recursão retornou erro (0 ms), throw não exportou texto (0 ms). Runtime novo após
falhas retornou string esperada. Configuração: heap 8 MiB, stack 128 KiB, interrupt
com deadline 100 ms por caso. Cargo tree conferido sem features rust-alloc/allocator
do rquickjs, loader/dyn-load/futures. Dependências baixadas do registry Cargo para
o spike; fontes documentais públicas docs.rs consultadas para rquickjs/Boa/Oxc.
Nenhum request ao YouTube, sessão lida ou áudio. Nenhuma mudança de código/dependência
do VibeDeck; suítes do app não repetidas. Erro foi colapsado, sem classificar causa:
tempo curto de alocação/recursão não demonstra sozinho OOM/stack específicos.
Não mede pico RSS, CPU sustentada, tamanho release, resistência a todo JS hostil,
cancelamento durante compilação/builtins ou compatibilidade com player real.
Protótipo temporário não é teste durável; próximo passo é integração controlada.

Revisão documental do decipher em 2026-10-21: Player, matchers, JsExtractor e
getNsigProcessorFn do YouTube.js no commit indicado em playback foram lidos como
fontes públicas GitHub. Definidos contrato planejado e casos sintéticos necessários;
nenhum JS do provedor executado/baixado, sessão lida, probe repetido ou código
alterado. Não há algoritmo ou sandbox implementado/validado. Suítes de código não
reexecutadas nesta mudança exclusivamente documental.

Tentativa real Gate B em 2026-10-21: autorização explícita para até um GET auxiliar,
um player e um SABR, sem busca/áudio/retry. Processo isolado já aberto, binário mais
novo que fontes relevantes, Vite/CDP disponíveis; não houve restart. Uma única
invocação de `debug_youtube_sabr_transport_once` via CDP na página isolada retornou
`sabr_probe_url_transform_required`. Pelo fluxo Rust, GET/player e bootstrap
concluíram; prepare_url rejeitou desafio s/n antes de construir/enviar SABR.
Não distingue s de n nem prova necessidade/suficiência de algoritmo específico.
Sem POST SABR, mídia, áudio ou seek; sem captura independente de tráfego. Tentativa
por processo consumida inclusive nessa falha; nenhum replay ou segundo invoke.
Diagnóstico contém só código fixo, sem URL/parâmetro/ID/token. Autorização encerrada.

Revisão pública em 2026-10-21: factory downloader, StreamerContext proto2 e
SabrStream do commit pinado conferidos, sem acesso ao YouTube/conta. ClientInfo
Rust agora omite locale/OS fabricados; teste existente confere campos 16/17 e
valores exatos. Revisão de decipher/cpn/token é leitura, não validação CDN;
ausência de desafio s/n/token não estabelece suficiência do contexto.
Após a mudança: 100 testes Rust lib, cargo check offline do binário, fmt check,
arquivos/links relativos e diff check passaram. Sem requests ao provedor, áudio
ou commit; frontend não alterado/retestado. Warnings dead-code persistem.

Integração explícita do probe em 2026-10-21: **100 testes Rust lib passaram**.
Cargo check offline do binário, fmt check, links relativos e diff check passaram;
warnings dead-code persistem. Frontend não alterado nem revalidado nesta rodada.
Dois novos testes sintéticos verificam preservação de query crua e bloqueio de
parâmetros assinados/challenges codificados, além do orçamento exato e rejeição
de campos extras. Nenhum comando de probe invocado, sessão real lida ou request
enviado. O handler debug teve impacto GitNexus LOW (zero processos/callers no grafo);
prepare_url e youtube_http::client não encontrados (UNKNOWN). Retry desabilitado
também no cliente compartilhado do bootstrap. Sem validação de política HTTP em
servidor ou de concorrência/consumo da tentativa via IPC real.

Preparação Gate B em 2026-10-21: 98 testes Rust lib passaram (três novos,
puramente sintéticos: destinos/query/challenges, framing/limites e projeção de
controles/proteção). Nenhum socket/servidor de teste, sessão real ou request
foi usado. O transporte `run_once` compilou, mas não foi invocado; política HTTP,
timeout e limites de leitura ainda não foram exercitados contra servidor.
Cargo check offline do binário, fmt check, links relativos e diff check passaram.
Frontend não alterado; testes/build frontend não repetidos nesta etapa.
GitNexus query/context/impact não localizaram o módulo/builder (risco UNKNOWN).
Módulo novo isolado, debug-only, sem IPC ou alteração no relay. Warnings dead-code
persistem. Não demonstra Gate B, áudio, continuidade ou seek.

| Escopo/data | Evidência | Limite |
|---|---|---|
| Antes das alterações Gate A, registro consolidado em 2026-10-21 | 64 Rust + 175 frontend, build/check/fmt passaram | Não descreve árvore atual |
| SABR/bootstrap, rodada anterior à correção final de cookies | 73 Rust lib, incluindo nove SABR | Sem aceitação de request ou som nativo SABR |
| Após correção de cookies/contexto | Build/check passaram | Suíte completa não repetida |
| Documentação, 2026-10-21, compactação anterior | `git diff --check` passou | Sem teste de código ou request |
| Filtro de domínio de cookies, 2026-10-21 | `cargo test --offline --manifest-path src-tauri/Cargo.toml --lib youtube_cookie_domain_tests`: 2 passaram; `cargo fmt` executado | Somente fixtures sintéticas; sem recaptura, request autenticado, bootstrap ou áudio SABR; suíte completa não executada |
| Extração de sessão nativa, 2026-10-21 | 80 testes Rust lib e 175 frontend passaram; build frontend, cargo check offline do binário e fmt check passaram | Sete testes de sessão incluem hashes, domínio, fallback, identidade opcional e roundtrip DPAPI com dados sintéticos em arquivo temporário removido. Sem leitura da sessão real, login, requests autenticados ou áudio; warnings dead-code e chunk frontend >500 kB persistem |
| Caminho JSON Ustreamer, 2026-10-21 | 81 testes Rust lib e 175 frontend passaram; build frontend, cargo check offline do binário, fmt check, links relativos e diff check passaram | Fixtures sintéticas no caminho cru confirmado em referência pública pinada; parser e probe compartilham helper. Sem leitura de sessão real, requests autenticados ou playback. Warnings dead-code e chunk >500 kB persistem; GitNexus não encontra os símbolos, mesmo após analyze informar índice atualizado |
| Sanitização do fetch/snapshot v2, 2026-10-21 | 89 testes Rust lib, 175 frontend, build frontend, cargo check offline do binário, fmt check, links relativos e diff check passaram | Testes só sintéticos: erros/JSON/status allowlistados, domínio raw WebView2, host-only, path boundary/prioridade, expiração, duplicatas/conflitos de auth, metadados e DPAPI com substituição de arquivo temporário. Legado protegido rejeitado sem reescrita. Sem ler sessão real, abrir login/WebView, enviar requests ou tocar áudio. Integração COM e política de redirects/limites não foram exercitadas contra servidor; dead-code/chunk >500 kB persistem |
| Ferramenta isolada de sessão, 2026-10-21 | 92 testes Rust lib, 175 frontend, 3 testes Node da página, build frontend, cargo check offline do binário, fmt check, links relativos e diff check passaram; sintaxe do launcher conferida | Comparação sintética de valores/metadados, ordem, ausentes/extras, allowlist de ações; página não invoca ao carregar, confirmação pode cancelar gravação e erros não expõem texto cru nem repetem operação. Nenhuma sessão real lida/escrita, login aberto, request autenticado ou áudio nesta implementação. Launcher/IPC/incognito/COM ainda não executados juntos; release não compilado nesta etapa. Warnings dead-code/chunk >500 kB persistem |
| Sessão real v2, 2026-10-21 | Usuário executou captura/save/reload e restore/readback pela ferramenta e forneceu os dois relatórios: 25 cookies, 24 persistentes, auth local elegível, comparação realizada, zero missing/changed/unexpected, matched=true | Evidência dos relatórios do usuário, não inspeção independente da jar. Zero host-only nesta sessão: esse caso permanece sintético. Não comprova aceitação do servidor, reinício ou sincronização contínua |
| Novo probe nativo de bootstrap, 2026-10-21 | Uma invocação autorizada de `debug_youtube_sabr_probe` via CDP/IPC na página isolada; ID selecionado do catálogo local em memória. Player OK, bootstrap parser ok, ABR presente, Ustreamer 944 bytes e quatro formatos áudio com lastModified | Zero formatos com xtags; Gate A estrito ainda não cumprido. Sem transporte SABR, som ou seek. Uma invocação do caminho GET auxiliar→POST player, sem retry/replay; sem captura independente do tráfego. Erro inicial no cliente CDP local (WebSocket indisponível no Node sem flag) ocorreu antes de qualquer invoke; corrigido com flag experimental, não repetiu player. Sem dumps de credenciais/IDs/payloads |
| Revisão offline de FormatId/encoder, 2026-10-21 | Leitura de bootstrap, FormatId, Writer, request builder e testes; `cargo test --offline --manifest-path src-tauri/Cargo.toml --lib sabr:: --quiet`: 10 passaram, 82 filtrados | Suíte existente, sem novos testes ou mudanças de código. Confirma omissão local de xtags vazio por leitura, não aceitação do servidor nem opcionalidade no schema upstream. Referência externa não consultada; sem sessão real, rede, playback ou seek. Warnings dead-code persistem |
| Conferência pública pinada de FormatId, 2026-10-21 | Schema proto2, encoder gerado, factory, formatUtils, SabrStream, TrackCollection e request schema consultados no commit documentado em playback; xtags opcional/omitido e matching normalizado confirmados por leitura | Supera bloqueio documental de xtags não vazio, sem novo player ou aceitação de transporte. Identificada divergência no estado inicial do campo 2 da request Rust; correção/testes pendentes. Somente fontes públicas GitHub, sem sessão, requests ao YouTube, playback ou mudança de código; testes não reexecutados nesta conferência |

GitNexus `detect_changes(scope=all)` nesta extração marcou a árvore acumulada como
critical (398 símbolos/16 arquivos), incluindo trabalho preexistente de UI/engine.
Não isola esta tarefa nem cobre completamente arquivo novo não indexado. Escopo
das edições desta etapa: módulo `youtube_session.rs`, adaptação de consumidores
em `lib.rs` e documentação responsável. Nenhum commit realizado.

Na etapa snapshot v2, `detect_changes(scope=all)` reportou critical para a árvore
acumulada (394 símbolos/16 arquivos), novamente sem isolar esta mudança nem cobrir
adequadamente os módulos novos. Revisão direta dos consumidores incluiu busca,
paginação, disponibilidade, login, player/probe e os dois bridges. Nenhuma alteração
de engine/UI nem commit nesta etapa; alterações preexistentes preservadas.

Observações históricas do provedor e suas limitações estão na seção de histórico
de [playback](playback.md); não equivalem a testes atuais. Lacunas de interação
por componente estão em [UI](ui.md).

## Histórico migrado da investigação (2026-10-22)

Trechos movidos literalmente de [investigação](investigation.md), sem revalidação.
São registros datados, não saldo nem autorização atual.

Inspeção estática passiva posterior via CDP, sem avaliar código alvo: o principal
do Chrome dedicado tem 2626522 bytes UTF-8, contra 2624029 nas tentativas Rust;
portanto as fontes não são idênticas. Não inferir a causa da diferença nem equivalência
de revisão. Os três scripts player não reportaram sourceMapURL no Debugger; os três
mapas falhos do bundle não estão demonstrados como mapas do decipher.
A única chamada .get('n') literal encontrada no principal está em helper que
sincroniza query n com o segmento /n/ da URL, sem transformar o valor. Uma referência
sintática ao helper foi encontrada; binding/cadeia dinâmica ainda não confirmados.
Outros literais n incluem categorias de controle, não necessariamente parâmetro de URL.
Nenhuma dessas observações autoriza substituir o matcher nativo.

Tentativas 1 e 2 retornaram player OK, mas urlEntries=0 e
trial_url_entry_not_unique; pararam antes do transporte. Aceitar três ou mais
parâmetros (conforme referência pública) passou teste sintético, mas não resolveu
a tentativa 2. Tentativa 3 contou apenas um inicializador de função, sem candidato.
Reconhecimento ampliado a Function AST (inclui atribuições/declarações), mantendo
marcadores/defaults, encontrou uma entrada única na tentativa 4; nInputs=0.
Tentativa 5 confirmou zero chamadas com argumento literal n nessa entrada.
Contagens globais: 12409 funções, 1976 com >=3 parâmetros, 46 com par de defaults,
3 com chamada alr/yes direta, 497 com construtor direto, 2 com ambos marcadores.
Isso é reconhecimento sintático, não prova de binding ao fluxo executado.
Tentativas 3–5 pararam antes do player. O rastreio offline de helpers/valores
indiretos ainda não avançou: o contrato literal .get('n') não cobre esta entrada;
fontes reais foram descartadas, sem dump. Feature debug opt-in native-playback-trial
mantém o resolver de produção e relay intactos. Objetivo primeiro trecho audível,
não faixa completa, seek ou renovação. Implementação/testes offline em andamento.

Checkpoint seguinte autorizado pelo usuário: **um GET Music novo + um download do
script declarado nesse mesmo bootstrap**, 20 s/4 MiB por GET, sem redirects/retries,
execução JS, POST player/SABR ou áudio. Alocação separada da inspeção estrutural
(ledger 2/20 preservado), não autorização para repetir falhas. Reserva persistente
create_new antes da sessão em bootstrap-script-checkpoint-20261022.count; uma
tentativa por checkpoint mesmo após restart, sem reset. Preparação offline concluída:
seleção AST exata, rejeição de cobertura parcial/duplicatas/ancestrais duplicados,
destino root-relative no origin Music sem cookies no script. Inventário de propriedades
signatureTimestamp com literais inteiros limitados, somente contagens, não valores.
Checkpoint consumido em uma invocação: bootstrap 793.317 bytes e script 2.624.029
bytes, script_diagnostic_inspected. AST encontrou **duas propriedades
signatureTimestamp, ambas com literal inteiro limitado**. Projeção não compara
valores nem classifica contexto/ancestrais: não se sabe se são iguais ou se uma
delas é a declaração relevante. Fonte/HTML/valores descartados sem dump; nenhuma
execução JS, POST player/SABR ou áudio. Evidência de aquisição pela declaração
sintática do bootstrap novo, não identidade global de ytcfg ou binding ao player.
Alocação consumida: 1/1 GET Music + 1/1 script; ledger estrutural continua 2/20,
18 restantes somente para raiz Music. O consentimento do download foi consumido.
Próximo passo offline: inventário de contextos/valores distintos sem
exportá-los, testes sintéticos e seleção conservadora de timestamp. Uma eventual
nova aquisição tem escopo próprio; os dois literais não foram promovidos a STS único.

GET ordinal 1: cobertura parcial, um script de 706.114 bytes pulado; uma propriedade
jsUrl literal/canônica em chamada direta ytcfg.set no caminho exato
WEB_PLAYER_CONTEXT_CONFIGS → WEB_PLAYER_CONTEXT_CONFIG_ID_MUSIC_WATCH.
GET ordinal 2, após restart autorizado com teto diagnóstico 1 MiB: 798.866 bytes
HTML, 17 scripts inline, sete externos excluídos e uma subárvore excluída;
maior inline 700.749 bytes, zero oversized/falhas JS/membros config não suportados,
partial=false e scriptCountLimited=false. Permaneceu exatamente uma propriedade
estática jsUrl, literal string canônica e no mesmo caminho Music Watch; ambiguous=false,
bindingValidated=false. Evidência html_ast_syntax_only, não identidade global/execução
de ytcfg nem vínculo ao POST player. HTML e valores descartados, sem dump/fixture real.
Aquisição limitada posterior e resultado de timestamp estão descritos no checkpoint
acima; plano de vínculo bootstrap→script→player permanece pendente.
Novo download de script/POST player/execução remota continuam fora desta alocação;
não há GETs adicionais da raiz planejados apenas para repetir a confirmação.

Checkpoint estrutural anterior: até três GETs autenticados raiz
Music (um previsto e até dois adicionais se necessários), uma inicialização/restart,
20 s/4 MiB por GET, sem redirects/retries automáticos; zero iframe_api/script/player/
SABR/áudio/seek/busca/escrita. Preparado IPC debug novo e independente
debug_youtube_bootstrap_structure_once, que executa somente um GET e reserva uma
tentativa própria antes da sessão. Não reabre guards históricos. Chamadas extras
não são retries cegos nem permitem contornar inelegibilidade; ainda não usadas.
Executado: um restart isolado (--no-watch) e um GET. Primeiro cliente CDP falhou
sem resultado; reconexão na mesma instância retornou bootstrap_structure_script_limit.
A trava única ainda disponível nessa reconexão indica que o primeiro cliente não
reservou a tentativa Rust. Limite não distingue fonte >256 KiB de >128 scripts;
nenhuma classificação jsUrl retornou. HTML descartado, sem dump/fonte para reanálise.
Guard estrutural consumido, sem reset. Dois GETs da margem não usados,
naquele checkpoint o restart havia sido consumido; autorização posterior acima
liberou restarts sem zerar requests. Guard histórico não reaberto. Feature opt-in bootstrap-inspection liga
somente fachada de parsers; não inclui QuickJS no app. Contexto sintático reconhece
ytcfg.set direto e caminho fixo WEB_PLAYER_CONTEXT_CONFIGS →
WEB_PLAYER_CONTEXT_CONFIG_ID_MUSIC_WATCH → jsUrl, sem prova de execução/binding.

Seleção HTML offline implementada em bootstrap_html.rs na crate isolada: parser
HTML com scripting enabled, rejeição de erros de parse e inventário de scripts
classic inline. Exclui subárvores template/noscript/foreign, scripts externos,
JSON/module/nomodule e language fora do subconjunto. Retorna apenas contagens,
incluindo duplicatas e falhas JS; não seleciona candidato nem aprova aquisição.
Contexto/IPC integrados em feature opt-in, sem executor JS no app. Inspeção real
inicial parou por limite; resultado contado atual acima superou essa limitação
diagnóstica. Decipher/transport/áudio continuam bloqueados.

Análise estrutural offline iniciada: classify_js_url_properties na crate isolada
classifica propriedades de objetos por AST (strings literais, outros valores,
accessors/métodos e chaves literais computadas), sem retornar valores ou escolher
candidato. Não presume ytcfg.set. Seleção HTML preparada separadamente; ainda falta
classificação semântica do contexto/ancestrais; código morto/aninhado também conta.
IPC estrutural preparado, resolver não alterado. HTML real descartado: localização
real agora confirmada apenas sintaticamente; resultado do checkpoint atual acima.

Retomada em 2026-10-22: usuário autorizou prosseguir na referência pública e concedeu
consentimento geral para requests autenticados/restart/playback. Ainda sem contagem,
duração ou operação definida: apresentar checkpoint numérico antes de executar;
essa concessão não reabre tentativas históricas nem autoriza retries indefinidos.
Checkpoint contado aprovado posteriormente: uma inicialização isolada se necessária
e exatamente um GET autenticado Music, até 20 s/4 MiB, sem redirects/retries,
player/iframe_api/script/SABR/áudio/seek/busca/escrita. Diagnóstico lexical preparado
e 116 testes Rust passaram. Executado uma única vez: bootstrap_probe_inspected,
coverage lexical_only, 803757 bytes, quatro marcadores crus de caminho, zero escapados;
jsUrl teve uma ocorrência string e um caminho candidato distinto. PLAYER_JS_URL/js/STS
sem matches nas formas pesquisadas; contagens não truncadas. Não é declaração
validada ou vínculo ao player. HTML/valores descartados, sem fonte disponível para
reinspeção offline. Orçamento encerrado, sem retry/saldo. Uma conexão CDP anterior
parou em ipc_not_ready antes do invoke; depois prontidão local foi aguardada na mesma
instância, sem nova inicialização ou GET extra. Instância usa --no-watch e permanece
aberta. Nenhum POST player, iframe_api, script, SABR ou áudio enviado.
Reconferência pública de Player/Session/Innertube pinados: revisão vem de iframe_api
antes da aquisição de base.js; timestamp extraído é enviado no pedido player.
Não confirma PLAYER_JS_URL no HTML Music nem assets.js na resposta. Detalhes e
diferença player_es6 versus getter player_ias em playback. Próximo trabalho offline:
preparar classificação estrutural limitada da localização de jsUrl no HTML/config
do mesmo bootstrap, com falsos marcadores/ambiguidade e revisões conflitantes
rejeitados. Match lexical não autoriza substituir o resolver; uma nova inspeção
estrutural real exigirá orçamento próprio. O IPC antigo só aceita assets.js e o novo
comando já foi consumido.

Peças entregues nesta etapa: invoke_synthetic tipado na crate isolada e transform_n
puro em sabr/url_resolution.rs. Nenhuma está conectada a requests reais; o probe
continua bloqueado. Próximo bloqueio concreto é obter/vincular o script de forma
limitada e completar validação/reconstrução antes de ligar essa execução ao app.
Preparado módulo isolado player_script.rs, sem invocação: vínculo por empréstimo
ao payload com assets.js, GET limitado e erros fixos. Campo/origem real ainda não
confirmados; nenhum fallback a revisão adivinhada. Falta coordenador com orçamento,
validação/reconstrução e integração. Bloqueio local de rlibs foi superado nesta
rodada com incremental desabilitado somente no comando, sem apagar cache/restart.
Os quatro testes de aquisição passaram. Coordenador debug-only do script agora
reserva uma tentativa por processo com orçamento exato de um GET, incluindo falhas,
sem reset. IPC debug-only adicionado com janela main, orçamento completo exato e
tentativa própria antes do catálogo/sessão. A autorização nova para 1 GET auxiliar,
1 player e até 1 script foi usada uma vez em 2026-10-22 na instância isolada cuja
inicialização foi autorizada separadamente. Retorno: playabilityStatus OK,
player_script_identity_missing, scriptBytes 0. Pelo fluxo, GET/player concluíram;
identidade faltante interrompeu antes do script. Não houve fonte adquirida,
execução remota, SABR ou áudio. Sem captura independente de tráfego.
Tentativa encerrada, sem retry nem reutilização do GET de script não enviado.
Próximo trabalho offline: definir origem verificável do script no bootstrap,
sem adivinhar revisão nem baixar iframe_api por conta própria; depois completar
efeitos/reconstrução. Não há autorização de requests/áudio aberta. Instância isolada
permanece aberta.
Etapa offline seguinte: launcher de desenvolvimento ainda ativo, portanto código
src-tauri não foi editado para evitar restart automático. Reconhecedor AST isolado
em tools/decipher-eval/src/bootstrap.rs preparado com fixtures hipotéticas de config
inline; não é parser HTML nem correção integrada. Origem real ainda não confirmada
pelas referências locais. Antes de integrar: obter evidência da declaração/origem
(referência pública específica ou nova inspeção limitada autorizada), definir vínculo
da config ao mesmo bootstrap e selecionar HTML/script sem aceitar falsos marcadores.
Não há novo request autorizado nem acesso remoto nesta etapa.

### Revisão estática em 2026-10-21

- Snapshot v2 e consumidores migrados para escopo/validade, com adaptador WebView2 direto; contratos e legado em [arquitetura](architecture.md). O header antigo não é convertido nem regravado: exige captura interativa futura. Arquivo real não foi lido/migrado nesta etapa. Fixtures/DPAPI não validam captura/restauração nativas nem aceitação pelo serviço.
- Divergência dos hashes corrigida em `youtube_session.rs`, seguindo a [referência yt-dlp consultada](https://github.com/yt-dlp/yt-dlp/blob/master/yt_dlp/extractor/youtube/_base.py): cookie por esquema, fallback para 3P e identidade opcional incluída no hash antes de acrescentar `_u`. Referência móvel, não contrato do serviço; fixtures não confirmam aceitação ou causa do bloqueio. Consumidores usam identidade ausente, não inventam identidade de conta.
- Parser e probe agora compartilham extração de `playerConfig.mediaCommonConfig.mediaUstreamerRequestConfig.videoPlaybackUstreamerConfig`. Mapeamento JSON cru confirmado no [parser YouTube.js pinado](https://github.com/LuanRT/YouTube.js/blob/78375cbf808544d91b0d5df99529a20f4a5a18eb/src/parser/parser.ts#L420-L441), que normaliza o campo usado pela referência SABR. Fixtures sintéticas corrigidas cobrem ausência, tipo inválido, vazio, base64 inválido e campo no local antigo; não há fallback para `streamingData`. Corrige falso negativo local possível; não explica status `UNPLAYABLE` anterior nem comprova shape/aceitação atual do serviço.
- Fetch agora compartilha cliente por operação, bloqueia redirects, limita corpo/tempo, rejeita contexto visitor/versão vazio e expõe somente erros fixos/status allowlistado. Logs de login não incluem mais URL/título. Não há jar de Set-Cookie GET→POST; AuthUser=0/logged-in=true e regex de config continuam limitações. Contexto adicional/signatureTimestamp permanece hipótese; nenhum campo foi adicionado por tentativa cega.
- Sanitização desta etapa cobre fetch de player/probe e logs de login; logs e erros legados de outros comandos ainda exigem revisão própria antes de executá-los como investigação.
- `npx gitnexus analyze` executado: informou índice atualizado, mas `fetch_youtube_player` não foi encontrado e contexto de auth contém referências anteriores. Cobertura do grafo é limitada; leitura estática foi necessária. Nenhum teste autenticado ou playback nesta análise.

### Resultado novo e próximo bloqueio (2026-10-21)

- Probe executado uma vez via IPC Rust na entrada isolada, com ID do catálogo local
  selecionado só em memória. Caminho de código: um GET auxiliar → um POST player;
  sem search, biblioteca remota, transporte SABR ou reprodução. Nenhum replay.
- `playabilityStatus=OK`, motivo `none`, sem errorScreen; server ABR presente com
  host googlevideo; Ustreamer decodificado com 944 bytes; quatro formatos áudio,
  todos com lastModified/signatureCipher, zero com xtags/URL direta; duração presente,
  poToken com zero bytes. Parser de bootstrap retornou `ok`.
- O parser aceita xtags ausente como string vazia. Isso não atende literalmente ao
  Gate A documentado e não demonstra que o transporte aceita esse identificador.
  Revisão local do encoder concluída; detalhes em [playback](playback.md#identificador-de-formato--revisão-local-em-2026-10-21).
  Próximo passo: conferir schema/encoder da referência pública pinada antes de qualquer Gate B;
  xtags não é fabricado e a dispensabilidade de poToken não foi demonstrada. O
  transporte tem autorização separada, e o player não é repetido por conta própria.

### Conclusão da revisão offline (2026-10-21)

O Writer omite xtags vazio; não há fabricação de valor. Isso não comprova que o
identificador sem campo 3 seja aceito pelo servidor. O bootstrap também aceita
lastModified inválido como zero, e os testes atuais não inspecionam esses limites.
GitNexus não retornou o fluxo nem encontrou FormatId; conferência feita no código
local, sem alegar cobertura do grafo. Nenhum código de produção foi alterado.
Gate A permanece pendente, não reprovado pelo servidor por xtags. Antes de propor
transporte: conferir opcionalidade/defaults no schema e tratamento na factory,
encoder e matching de formatos da referência pinada; definir testes de bytes e
validação de lastModified. Consulta pública às fontes pode ser feita separadamente,
sem sessão/requests ao YouTube; não foi realizada nesta revisão estritamente offline.

### Próximo trabalho após conferir a referência pública

Conferência de schema, encoder, factory, matching e estado de inicialização
concluída, sem acesso à conta; fontes e limites em [playback](playback.md).
A conclusão anterior de Gate A pendente por xtags foi superada pela opcionalidade
confirmada na referência, não por um novo player. Correção offline concluída:
initialization_format_ids vem de lista explícita inicialmente vazia; bootstrap
descarta identificadores sem itag/lastModified positivos. Testes de bytes/defaults,
estado inicial/confirmado/reset e lastModified inválido passaram; detalhes em
[playback](playback.md) e [validação](validation.md). A atualização da lista por
metadata pertence ao coordenador futuro, ainda não implementado.
Probe de transporte debug-only preparado em `sabr/transport_probe.rs`, com IPC explícito:
validação de URL, limites de tempo/corpo/partes, erros fixos e sem seguir
redirects/reload/retry automaticamente. Testes sintéticos de framing estrito e
projeção passaram; resultado da tentativa real acima. Limitações em [playback](playback.md).
Query assinada preservada em bytes nos pares não alterados, com bloqueio de rn/alr
assinados. Comando aceita orçamento fixo GET/player/SABR e uma tentativa por processo,
incluindo falhas. Revisão pública pinada de contexto/decipher/cpn/token
concluída: contexto opcional fixo removido; cpn existente preservado; s/n bloqueiam;
sem mint/reload. Ausência de token pode resultar em proteção pendente/rejeitada,
sem provar causa única de falha. Detalhes e demais divergências em playback.
Revisão pública do decipher concluída por leitura do YouTube.js pinado: extração
estrutural de JS/dependências e execução são necessárias na referência; não há
evidência para um algoritmo fixo reverse/swap. Contrato planejado e matriz de testes
em [playback](playback.md#revisão-do-decipher-e-contrato-planejado-2026-10-21).
Spike temporário rquickjs 0.14.0 compilou em Windows: seis casos sintéticos passaram,
incluindo loop limitado por interrupt e falhas de alocação/recursão; detalhes e
limites em playback/validação. Não é sandbox aprovada nem decipher implementado.
Executor experimental durável em tools/decipher-eval, crate isolada do app: onze
testes offline passaram (resultados/limites em validação). rquickjs só foi adicionado
nessa crate; Cargo e fluxo de playback do app permanecem intocados nesta etapa.
Reconhecedor AST inicial em extraction.rs com Oxc 0.95.0: nove testes novos, total
experimental de 20. Extrai apenas slice de uma candidata única; não resolve escopo,
dependências nem executa função/contêiner. Limites do executor não se aplicam ao parser.
Resolver lexical conservador implementado separadamente em resolution.rs, com
inventário transitivo de bindings e rejeição de referências não suportadas;
contratos/limitações em playback. Não gera programa executável nem achata escopos.
Plano de reconstrução agora agrupa bindings por escopo e preserva ordem original,
sem achatar nomes iguais ou gerar código executável (reconstruction.rs).
Revisão estática identificou metadados ausentes: kind da declaração, posição do
inicializador/controle de fluxo e strictness. Escritas em propriedades, escapes e
ambientes dinâmicos ancestrais também não são cobertos pelo inventário atual.
Próximo passo: gate separado de elegibilidade para reconstrução, inicialmente
restrito a declarações diretas e efeitos classificados, antes de ampliar formas
suportadas ou gerar código. Matriz e limites em [playback](playback.md).
Definir também vínculo ao bootstrap antes de integrar.
Primeira etapa do gate em structure.rs restringe o programa inteiro a const diretas
com inicializadores simples; rejeita statements/contêineres externos. Não certifica
efeitos nos corpos/defaults nem autoriza execução. Próxima etapa: classificar esses
efeitos e inicialização temporal antes de gerar qualquer programa.
Análise seguinte: inicialização de const da raiz pode ser garantida chamando só
após todas as declarações aceitas, mas isso não cobre TDZ interna/defaults. Fixtures
positivas atuais não constroem h.URL e não são transformadores funcionais. Antes
de ampliar o gate, definir fixture coerente e inventário de efeitos por fase,
com chamadas/propriedades desconhecidas bloqueadas; roteiro em playback.
Fixture funcional sintética agora executa o original com stub local e argumentos
fixos. Teste de default com throw confirma que sucesso estrutural não certifica
efeitos na chamada. Próxima etapa continua sendo classificação por fase/ownership,
não integração do evaluator com o plano ou liberação de script remoto.
Gate de parâmetros implementado em defaults.rs: aceita somente bindings simples
e defaults literais primitivos em todas as funções; rejeita IIFE/defaults com
expressões, destructuring e rest. Próxima etapa: efeitos dos corpos, chamadas e
propriedades/ownership, mantendo bloqueio de geração e execução remota.
Nenhuma transformação ou integração com app implementada, nem download de player autorizado.
O bloqueio por tentativa cega permanece. Não foi distinguido
qual parâmetro acionou o guard (projeção fixa intencional). Um novo teste real
depende de nova autorização contada, incluindo renovação de bootstrap e eventual restart;
o saldo SABR da tentativa encerrada não é reutilizado. Gate B continua sem transporte validado.

## Checagem local da documentação

Correção offline do builder SABR em 2026-10-21: **95 testes Rust lib e 175
frontend passaram**, build frontend e cargo check offline do binário passaram.
Três novos testes sintéticos cobrem FormatId/defaults, estado de inicialização
e filtragem de lastModified; teste existente ajustado para campo 2 inicialmente
ausente. GitNexus impact não encontrou AbrRequestState/build_abr_request/
from_player_payload/teste existente (risco UNKNOWN); callers conferidos por leitura
direta: builder usado nos testes, parser também no probe debug em lib.rs.
Sem sessão real, requests, playback ou commit. Warnings dead-code e chunk >500 kB
persistem. Não demonstra aceitação de transporte ou atualização automática por metadata.

Executar no root do projeto, sem rede. Verifica arquivos obrigatórios e destinos
de links Markdown inline relativos (não valida anchors, links reference-style ou URLs externas).

```powershell
$files = @('AGENTS.md', 'README.md', 'CLAUDE.md', 'YOUTUBE-MUSIC-API.md', 'docs/README.md', 'docs/architecture.md', 'docs/playback.md', 'docs/ui.md', 'docs/decisions.md', 'docs/investigation.md', 'docs/validation.md')
$errors = @()
foreach ($file in $files) {
  if (-not (Test-Path -LiteralPath $file)) { $errors += "Ausente: $file"; continue }
  $text = Get-Content -Raw -LiteralPath $file
  foreach ($match in [regex]::Matches($text, '\[[^\]]*\]\(([^\s)]+)\)')) {
    $target = $match.Groups[1].Value
    if ($target -match '^(?:[a-zA-Z][a-zA-Z0-9+.-]*:|#|/)') { continue }
    $target = [uri]::UnescapeDataString(($target -split '#', 2)[0])
    if (-not $target) { continue }
    $base = Split-Path -Parent (Join-Path (Get-Location) $file)
    if (-not (Test-Path -LiteralPath (Join-Path $base $target))) { $errors += "$file -> $target" }
  }
}
if ($errors.Count) { throw ($errors -join "`n") }
git diff --check
if ($LASTEXITCODE -ne 0) { throw 'Diff inválido' }
```

### Pesquisa de clientes alternativos, 2026-11-05

Probe debug `debug_youtube_player_client_survey` (`src-tauri/src/sabr/client_survey.rs`): um `/player` POST por cliente,
sem cookies e sem Authorization, só contagens no relatório; uma execução por processo. Fontes públicas (guia de PO token
do yt-dlp) dizem que `tv` e `android_vr` não exigem token de GVS, que `web_music` exige, e que Premium dispensa.
Resultado contado (2 requests de player, mesma faixa do catálogo): `android_vr` e `tv` voltaram `LOGIN_REQUIRED`, zero
formatos, sem URL SABR, sem URL direta. Anônimo não basta para esta faixa; a causa (bloqueio de bot do IP, exigência de
conta para música) não é distinguível pelo status. Ainda não testado: `tv` com os cookies da conta do app.

Segunda rodada, `tv` autenticado (2026-11-05): 1 `/player` POST como TVHTML5 para `music.youtube.com`, com cookie e SAPISIDHASH da
sessão salva do app. Resultado: `UNPLAYABLE`, zero formatos, sem URL SABR, sem URL direta. A mudança de `LOGIN_REQUIRED` (anônimo) para
`UNPLAYABLE` sugere que a conta foi reconhecida, mas o motivo da recusa não é distinguível pelo status. Não testado: mesmo cliente no host
`www.youtube.com` com SAPISIDHASH do origin `https://www.youtube.com` (o helper atual só assina o origin de música).

Terceira rodada, sessão viva do navegador integrado (2026-11-05): `fetch` same-origin de dentro de `music.youtube.com` (logado pelo usuário), só
resumo estrutural. Controle `WEB_REMIX`: HTTP 200, `OK`, 20 adaptativos, 6 de áudio (todos com `signatureCipher`, nenhum com URL direta),
`serverAbrStreamingUrl` presente. `TVHTML5` na mesma sessão e mesmo vídeo: HTTP 200, `UNPLAYABLE`, zero formatos. Conclusão: o snapshot do app
não é a causa; o `tv` é recusado também com a sessão viva quando pedido pelo host de música. Ainda não testado: `tv` a partir de
`www.youtube.com`, e a condição de Premium da conta.

### Sessão completa aceita observada, 2026-11-05 (`session-observe.cjs`, conta com YouTube Music Premium)

Gancho passivo `scripts/sabr-trace/session-observe.cjs` (só números) no navegador integrado, uma faixa tocada (itag 140, AAC) até o fim da transferência.
- **Modo usado: UMP por faixa de bytes, não SABR com `StreamerContext`.** 9 POSTs a `/videoplayback`, corpo de 2 bytes (só campo 15), resposta com
  partes 20 (cabeçalho), 21 (mídia), 22 e 58 (estado de proteção = 1, ok). Sem partes 35/57/59, sem cookie de playback, sem contextos.
- **URL:** `c=WEB_REMIX`, `range=N-N`, `ump=1`, `pot`, `sig`+`lsig`, `n`, `alr`, `cpn`, `cver`, `rn` (10 a 18), `rbuf`, `srfvp`; `pot` fora do `sparams`.
- **Cadência:** três rajadas em ~1 s (66 KB, 66 KB, 132 KB), depois 66 KB, 221 KB, 323 KB, 485 KB, 485 KB e 276 KB; faixa inteira (~2,05 MB) em ~68 s,
  com a reprodução em 48 s e buffer de ~110 s. Os intervalos são ~15 s a partir do 6º pedido. Offsets contíguos, todas as respostas 2xx.
- **Não observado:** seek e pausa (offsets monótonos, nenhum salto); a 4ª resposta traz outro `lmt` e início em 0, provavelmente prefetch de outra faixa.
- **Leitura:** na conta Music Premium o player web continua enviando `pot`. A dispensa de token para assinantes não se verificou para este cliente.
  O modo por bytes é mais simples de agendar que o SABR completo (sem política nem contextos), mas tem o mesmo bloqueio de `pot`, mais `sig` e `n`.

Continuação da mesma observação (2026-11-05): 42 POSTs a `/videoplayback` em várias faixas, incluindo seek, **todos** com corpo de 2 bytes (campo 15), partes 20/21/22/58, 2xx.
Nenhum POST com corpo SABR completo. As faixas lidas eram `MUSIC_VIDEO_TYPE_ATV` (áudio de catálogo); a resposta `/player` dessas faixas traz `serverAbrStreamingUrl`
e 12 formatos de vídeo, e mesmo assim o player usou o modo por bytes. Seek aparece como novas rajadas de pedidos curtos a partir da posição nova, sem mecanismo extra.
Hipótese aberta: SABR completo só para músicas com vídeo (`OMV`) ou modo vídeo; ainda não testada.

Teste da hipótese "SABR só com vídeo" (2026-11-05): faixa `MUSIC_VIDEO_TYPE_OMV` em modo vídeo (`videoWidth` 640, 16 formatos de vídeo adaptativo, `serverAbrStreamingUrl`
presente). 16 POSTs novos, todos 2xx, **todos com corpo de 2 bytes (campo 15)** e partes 20/21/22/58, rajadas de vídeo e áudio por faixa de bytes. **Hipótese refutada** nesta
revisão do player: nem faixa com vídeo usou o corpo SABR completo. Total do gancho: 60 POSTs, nenhum SABR completo. As observações de 2026-10-22 (corpo SABR com `StreamerContext`
e token de 85 bytes) vinham de outra revisão/experimento do player; a conta e o cliente (`WEB_REMIX`) são os mesmos.

### Pedido por faixa de bytes sem `pot`, 2026-11-05 (1 request, de dentro do navegador integrado)

Repetição de um POST de mídia que o player já fez (mesma URL assinada, mesmo corpo de 2 bytes, faixa limitada a 64 KiB), com o parâmetro `pot` removido e **sem cookies**
(`credentials: omit`). Resultado: 2xx, `application/vnd.yt-ump`, 65.600 bytes, partes 20/21/22/58, 65.537 bytes de mídia. Limites da prova: um único pedido, faixa inicial, o valor
numérico da parte 58 não foi lido (nos pedidos do player o valor era 1). Não prova que a faixa inteira, faixas sem fila de reprodução ativa ou o IP nativo sejam aceitos sem `pot`.

Sequência sem `pot`, 2026-11-05 (10 requests, de dentro do navegador integrado, sem cookies): 10 faixas consecutivas de 256 KiB (offsets 0 a ~2,6 MB, `rn` crescente, 400 ms entre pedidos) de uma faixa de
áudio com `clen` maior que 1 MB, copiando a URL assinada do player e removendo só `pot`. Todos 2xx, `application/vnd.yt-ump`, partes 20/21/22/58, 262.144 bytes de mídia cada (2.621.440 no total),
**estado de proteção (parte 58) = 1 em todos**. O limite de 10 pedidos cobriu só o começo da faixa (a faixa é maior que 2,6 MB); o fim, o seek e a pausa não foram exercitados nesta sequência.
Segue sem prova para IP/TLS nativo e para obter a URL com `sig` e `n` fora do navegador.

### Pedido nativo por faixa de bytes sem `pot`, 2026-11-05 (1 request, cliente Rust do app)

Comando debug `debug_youtube_range_probe_once` (`src-tauri/src/sabr/range_probe.rs`, 3 testes offline): lê uma URL de mídia assinada que o player do navegador integrado produziu
(passada por arquivo local, apagado depois; área de transferência limpa), remove `pot`, envia 1 POST de 2 bytes (campo 15) pelo `reqwest` do app, sem cookies nem Authorization,
com `Origin`/`Referer` de `music.youtube.com`. Resultado: **2xx, UMP, 65.597 bytes, partes 20/21/22/58, 65.536 bytes de mídia, estado de proteção 1.** O cliente nativo (TLS, cabeçalhos, IP)
é aceito sem `pot`, na faixa inicial. Pendente: obter essa URL (`sig` decifrado e `n` transformado) dentro do app, sequência completa com seek, expiração/renovação, outras faixas.

### Custo do solver: preparo contra resolução, 2026-11-05 (`tools/decipher-eval/examples/ejs_split.rs`, build debug, player em cache de 2.662.178 bytes)

Mesmo QuickJS, entradas sintéticas. Carregar o solver: ~35 ms. **Preparo do player (parse + reescrita, `output_preprocessed`): 50,6 s**, gerando um script preparado de 3.747.016 bytes.
**Resolver `n` e `sig` juntos a partir do script preparado: 572, 610 e 573 ms** (saídas de 13 e 106 caracteres). Ou seja, o custo de ~55 s é quase todo preparo, que depende só da revisão
do player e pode ser feito uma vez e guardado em disco; cada faixa nova gasta ~0,6 s (em build debug sem otimização; release deve ser menor). Falta medir memória de pico e o build release.

### Cadeia nativa completa por faixa de bytes, sem `pot`, 2026-11-05 (`debug_youtube_native_media_once`, 1 GET + 1 player + 1 mídia)

Tudo dentro do app, sem navegador: `/player` autenticado do app para a primeira faixa do catálogo → formato AAC (itag 140, com `signatureCipher`) → `n` e `s` respondidos pelo yt-dlp-ejs a partir do
player **preparado e guardado em disco** (preparo de ~50 s só na primeira vez; depois resolução de ~0,6 s) → URL montada com `alr`, `cpn`, `cver`, `range`, `rn`, `rbuf=0`, `ump=1`, `srfvp=1`, sem `pot` →
1 POST de 2 bytes, sem cookies. Resultado: **2xx, UMP, 65.612 bytes, partes 20/21(×4)/22/58, 65.536 bytes de mídia, estado de proteção 1.**
Duas tentativas anteriores falharam de forma instrutiva e ficam como lição:
1. Sem `signatureTimestamp` no corpo do `/player` nativo, a resposta veio com `s` embaralhado para outra revisão do player; a decifragem saiu "válida" (108 caracteres) mas o servidor respondeu 2xx com
   **só uma parte de erro (44)**, 31 bytes, campo numérico 2 = 4, sem mídia. Um erro de assinatura não aparece como 401/403 neste modo.
2. Faltavam `rbuf` e `srfvp` na URL; adicioná-los não mudou o erro (a causa era o timestamp), mas ficam por espelharem o player.
Com `signatureTimestamp` igual ao declarado no player (20759), o `n` passou de 18 para 14 caracteres e o `s` manteve 104. Pendente: faixa inteira com 10+ pedidos no nativo, seek, renovação da URL (~6 h),
aquisição do script do player pelo app (hoje vem do cache de laboratório), outras faixas, e medir memória/build release do preparo.

### Agendador por faixa de bytes com seek, 2026-11-05 (`debug_youtube_native_playback_once`, 1 GET + 1 player + 8 pedidos de mídia de 14 permitidos)

Reprodução AUDÍVEL pelo motor do app, só com pedidos nativos (sem `pot`, sem cookies na mídia). Fluxo: cabeçalho `0..=indexRange.end` (1.102 bytes, `sidx` com 24 segmentos, duração 231.107 ms) →
pedidos de 192 KiB mantendo ~30 s à frente da posição, espaçados em ≥350 ms, com `rbuf` real → 12 s de reprodução → seek a 50% (alvo 115.553 ms, segmento 11): reaproveita o cabeçalho (sem rede) e busca
os segmentos a partir do alvo → reinício do motor. Resultado: **`stage=done`, sem erro; 4 pedidos antes do seek e 3 depois; 44 amostras**: a posição do motor acompanhou o relógio (≈1 ms por ms) tanto antes
quanto depois do seek (0 → ~12 s, depois recomeça em 0 e avança até ~8,5 s). O áudio após o seek vem do segmento 11. Conferência auditiva do usuário: tocou do início por ~10 s e depois saltou para um ponto que lhe pareceu perto de 1:30 da música (o alvo era 1:55; o segmento 11 começa antes do alvo, na granularidade de ~10 s do `sidx`).
Falhas anteriores desta sessão de testes, para não repetir: (1) o motor recusa reativar com o mesmo `activation_sequence` ("geração/intenção obsoleta"); o reinício do seek precisa de sequência +1;
(2) o script de teste esperava 540 s por um temporizador pendente e cortava o início do relatório com `Select-Object -Last`; agora grava o relatório em arquivo e encerra.
Limitação conhecida: o motor não aceita deslocamento inicial, então a posição exibida recomeça em 0 após o seek (o tempo real seria `alvo + posição`). Pendente: pausa/retomada, fim de faixa, expiração/renovação da URL, aquisição do player pelo app.

### Aquisição do player pelo app, 2026-11-05 (`debug_youtube_native_media_once`, 3 GETs + 1 player + 1 mídia)

O app deixou de depender do cache de laboratório: `acquire_player` faz 1 GET da página de música com os cookies do app, lê o `jsUrl` com o inspetor estrito (`select_music_script`: só
`/s/player/<id>/<variante>/base.js`), e, se a revisão não estiver em `%LOCALAPPDATA%\VibeDeck\player-source` (TTL 7 dias, 3 revisões), faz 1 GET do script **sem cookies** (MIME JavaScript, ≤ 6 MiB,
precisa conter `signatureTimestamp:`). O `/player` repete a página internamente (3º GET). Resultado: script baixado de 2.662.178 bytes (mesmo conteúdo do cache de laboratório, então o player preparado foi reaproveitado
do disco), `signatureTimestamp` lido do próprio script, `n` 18→14 e `s` 104 em 661 ms, e 1 POST de mídia: **2xx, UMP, 65.597 bytes, 65.536 de mídia, proteção 1**. O orçamento das duas sondas passou a exigir `auxiliaryGet: 3`
(teto; o uso real vem em `auxiliaryGets`). Pendente: compartilhar a página entre o GET de aquisição e o do `/player` (uma requisição a menos), troca automática ao mudar a revisão em reprodução longa.

### Pausa, renovação e fim de faixa, 2026-11-05 (`debug_youtube_native_playback_once`, 6 pedidos de mídia de 20, 2 `/player`, GETs ≤ 5)

Regra de decisão pura em `range_policy.rs` (6 testes): pedir, esperar, renovar ou encerrar, a cada passo. Execução AUDÍVEL com pausa de 5 s, seek a 97% (segmento 22, alvo 224.173 ms) e corrida até o fim:
- **Pausa:** 0 pedidos de mídia durante os 5 s; posição do motor congelada (8.042 ms por ~5 s); ao retomar a posição voltou a avançar (+~4,2 s em 4 s).
- **Renovação forçada** após 4 pedidos: 1 renovação (novo `/player`, novas respostas de `n` e `sig`), sem erro e sem respostas 401/403; os pedidos seguintes usaram a URL nova.
- **Seek:** reinício com cabeçalho reaproveitado + segmento 22; posição recomeçou em 0 e avançou ao ritmo do relógio.
- **Fim de faixa:** o agendador esgotou o plano, fechou o canal e o motor terminou sozinho em 3,6 s (`endedClean`), sem erro e sem motor ativo no fim.
Pendente: renovação por 401/403 real (o caminho existe e está coberto por teste da política, mas nenhuma resposta negada ocorreu), outras faixas/formatos (opus), posição exibida após seek, integração ao comando de reprodução da interface.

### Integração ao app, teste manual no `tauri dev --features native-range-playback`, 2026-11-05

Reprodução de faixas do YouTube da biblioteca do usuário pelo caminho nativo por faixa de bytes (sem relay, sem WebView de transporte, sem `pot`, sem cookies na mídia): tocar do início ao fim, pausa e retomada, seek pela barra. O console distingue os caminhos
(`VibeDeck playback path=native_byte_range` ou `path=continuous_sabr_relay`); só o nativo tem seek.
Defeito achado no teste e corrigido: (1) a posição após o seek ficava no início do segmento (até ~10 s antes do alvo); a interface via a diferença (> 0,35 s) e reenviava o seek, reiniciando o fluxo em ciclo de ~1 s. O motor agora descarta o áudio
decodificado entre o início do segmento e o alvo (`skip_seconds`) e a posição nasce exata. (2) Arrastar a barra gerava um seek completo por passo; a sessão junta os alvos que chegam em 300 ms e reinicia uma vez no último (`merged_requests` no console),
e o reinício busca 1 pedaço antes de voltar a tocar. Resultado confirmado de ouvido pelo usuário: seek por clique e por arrasto soam corretos e a resposta da interface melhorou. Registro do console da sessão: 18 seeks, 14 isolados e os demais
juntados (2, 4 e 7 alvos por reinício).
Limites: testado só em Windows, build de debug, faixas de áudio de catálogo em AAC; o preparo do player de uma revisão nova leva ~50 s em debug; a renovação por 401/403 real e outros formatos (Opus) não foram exercitados na interface.

### Biblioteca e playlists nativas, WebView `youtube-library-bridge` aposentada, 2026-11-05

Leitura da biblioteca (`FEmusic_liked_playlists`) e das faixas (`VL<id>` e continuações) por `browse` autenticado em Rust (`youtube_browse.rs`), com a sessão do app (cookie + SAPISIDHASH). Sonda: biblioteca 66 KB (7 itens de grade, sem continuação), playlist 1,2 MB (100 faixas e 1 continuação). No app (`tauri dev`), o usuário confirmou de olho a listagem de playlists e de faixas; o console registrou `library path=native_browse playlists=6`.
Removido: a janela `youtube-library-bridge`, seus scripts injetados, os handlers de título e os estados estáticos associados. Sem fallback: falha de rede ou sessão vira erro claro para a interface. Testes: `cargo test --lib` 164 e `vitest` 179 passando.
Pendente: o relay `youtube-stream-bridge` (só Windows) ainda é o fallback de reprodução; a paginação de faixas de playlists grandes (> 100) só foi exercitada pela continuação já existente.
