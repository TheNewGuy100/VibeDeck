# Investigação ativa — SABR nativo

Atualizado em 2026-10-22: histórico de tentativas e resultados datados migrado para
[validação](validation.md#histórico-migrado-da-investigação-2026-10-22). [Índice](README.md).

## Estado e bloqueio

Em 2026-10-21, um novo probe nativo autorizado retornou player `OK` e bootstrap
aceito pelo parser, sem xtags. Conferência pública pinada confirmou que xtags é
opcional: Gate A estrutural atendido segundo o critério revisado abaixo, sem provar
transporte. Builder corrigido; probe Gate B registrado no IPC debug, nunca invocado automaticamente. Três autorizações históricas de player foram consumidas;
a quarta histórica foi interrompida, não concedida. A autorização nova desta conversa
(um GET auxiliar e um player, sem retry/playback) foi consumida por uma invocação.
**Não há autorização aberta fora dos checkpoints contados descritos abaixo.**

Tentativa diagnóstica Gate B autorizada posteriormente (até um GET + um player +
um SABR) consumida em uma invocação: retornou `sabr_probe_url_transform_required`.
Pelo caminho de código, GET/player e bootstrap concluíram; preparação de URL
bloqueou s/n antes do POST SABR. Zero transporte SABR/áudio; sem captura independente
de tráfego. Tentativa por processo consumida.
Bloqueio atual: transformação da ABR URL ainda não integrada. O guard distingue
`s`, `n` e ambos (`sabr_probe_url_transform_required_{s,n,s_n}`). Em 2026-10-22, uma
tentativa contada do Gate B (1 GET + 1 player + 1 SABR, processo novo) retornou
`_n`: a ABR URL traz `n` e não traz `s`, então a transformação do `n` é necessária
para o transporte, e o bloqueio do extrator é real. Tentativa por processo consumida.
Leitura em memória do player no navegador integrado (2026-10-22, só estrutura): dos 14
literais `"n"`, apenas um é de URL, num helper que copia o `n` da query para `/n/` e só é
chamado para URLs de HLS. A `serverAbrStreamingUrl` é guardada crua num objeto de URL,
sem transformação visível nesse ponto. A classe de URL usa tabela de strings `w[...]` e
delega `get`/serialização a funções de fluxo achatado com seletor numérico; `"n"` está na
tabela sem uso literal direto. Isso explica por que o matcher por literal não acha o `n`.
Hipótese de que o player web não transforma o `n` para SABR: **refutada pelo trace do navegador**
(2026-10-22, ver [validação](validation.md)). A `serverAbrStreamingUrl` recebida traz `n` com 16 ou
mais caracteres e o POST SABR do navegador envia `n` com no máximo 15; ou seja, o cliente
transforma o valor antes de enviar. Em nosso lado, o `n` cru levou a `sabr_probe_http_access_denied`
(401/403), compatível com isso, mas a mesma request também não leva PO token (abaixo).
Transformação do `n` (2026-10-22): **o candidato anterior foi refutado.** A leitura estática achou, no
objeto `pU` e na função `Gh`, oito operações (troca, corte, inversão) resolvidas com `d = 2471`. O oráculo
(solver do yt-dlp, `yt-dlp-ejs` 0.8.0, num QuickJS isolado sobre o script em cache de 2.626.523 bytes, 59 s)
deu resultado diferente nas 6 entradas sintéticas, e as saídas contêm caracteres que não estão na entrada,
o que uma permutação pura não produz. Essas operações são o formato clássico da decifragem da assinatura
`s`, que a URL SABR não usa: foi erro de identificação meu. O que o oráculo mostra é que a saída tem 5
caracteres a menos que a entrada, coerente com o trace (`n` recebido com 16 ou mais, enviado com no máximo
15). O solver acha a função de URL pela âncora `.set("alr","yes")` (a mesma do nosso reconhecedor), faz
`url.set("n", n)`, chama o primeiro método não padrão da URL e lê `url.get("n")`, ou seja, executa o código
do player. O script em cache é de outra variante (2.626.523) que a do navegador integrado (2.624.029), e
o solver **foi validado por valor contra o navegador** (2026-10-22, um play no Chrome do mesmo script em cache): os 2 `n` recebidos, passados pelo solver no QuickJS, deram exatamente os 2 `n` que o POST SABR enviou (19 caracteres viram 14). Isso prova o oráculo para essa variante, mas ele leva cerca de 55 s no QuickJS, caro demais para o app; falta uma implementação nativa e leve.
PO token, leitura em memória do mesmo player (2026-10-22, só estrutura): o token é **gerado no
cliente**, não entregue pelo servidor. O player usa o serviço de atestação (`Waa/Create` para o
programa do BotGuard, execução do snapshot e `Waa/GenerateIT`), com flags de token de sessão e de
conteúdo e troca por vínculo de vídeo. O valor é anexado em três lugares: no corpo do `/player`
(`serviceIntegrityDimensions.poToken`), no campo 2 do `StreamerContext` do SABR (somente se
existir; o código não o torna obrigatório) e como `pot` nos pedidos DASH por GET. O servidor
decide se exige. No lado nativo, `lib.rs` e `sabr/mod.rs` leem o token do **payload de resposta**
do player, e o pedido nativo não o envia: o poToken de zero bytes é esperado, não um defeito do
servidor. O trace do navegador confirmou que o POST SABR real **leva o campo 2 do `StreamerContext`**
(classe 16–255 bytes) em todas as requests, e que a resposta do `/player` não traz
`serviceIntegrityDimensions`. O servidor aceitou essas requests (2xx, `ump`), mas não foi
demonstrado que ele recuse a ausência do token. Tentativa contada de 2026-10-22 (ver
[validação](validation.md)): com `n` transformado e formato espelhado, **sem token**, a primeira
request SABR devolveu 2xx com mídia; o 401/403 anterior vinha do `n` cru e/ou do formato. Ainda
aberto se o token passa a ser exigido em requests posteriores, em seek ou em outras faixas. O corpo do `/player` do navegador não pôde ser lido
(vem comprimido), então não se sabia se o token também ia nele. Observado depois no player real
(ver [validação](validation.md)): o `/player` leva um `poToken` curto (10 bytes) de sessão; o SABR leva um token
de 85 bytes por faixa, gerado localmente por um gerador reaproveitado, sem chamada de rede por faixa.
Diferenças estruturais do POST SABR do navegador contra o nativo, só formato: a URL ganha `cpn`,
`cver`, `rn` e `alr`; o corpo não tem o campo 16 (o nativo envia) e traz `ClientInfo` com os
campos 1, 16, 17, 18 e 19 (o nativo envia 16 e 17); o navegador não define cabeçalhos próprios.
Contagens do script de player em [validação](validation.md). Inspeção nova em
2026-10-22 retornou player OK e player_script_identity_missing: assets.js ausente
ou sem string no payload, portanto nenhum GET de script pelo caminho implementado.
Origem do vínculo à revisão precisa ser revista offline antes de outra tentativa.

Snapshot v2 foi recapturado/restaurado pelo usuário, com relatório sanitizado de
25 cookies idênticos. O novo player foi bem-sucedido. Causa histórica de `UNPLAYABLE`
continua desconhecida: a categoria `unavailable` isolada não distingue auth inválida,
bot-check, PO token ou indisponibilidade.

## Evidência necessária para retomar sem repetir trabalho

- Três requests em 2026-10-20, sobre dois IDs (catálogo e log antigo de resolução), retornaram `UNPLAYABLE`, zero formatos e ausência de server ABR URL/config. A terceira classificou `unavailable`, com errorScreen e duração. Log de resolução não prova playback daquele ID.
- Antes da correção: versão/contexto presentes; root GET anônimo, `hl=en`/`gl=US`; snapshot guardava dez nomes. Jar viva tinha 35 cookies do domínio, incluindo PSID/PSIDTS/PSIDCC omitidos. Nomes não demonstram validade/igualdade de valores.
- Depois: captura removeu allowlist, mantendo filtro de domínio/dedup por nome; root GET envia cookie; GL/HL vêm da página com fallback US/en. Snapshot protegido cresceu de 1.014 para 2.582 bytes. Isso não demonstra bootstrap aceito.
- Hashes foram corrigidos offline no módulo nativo de sessão; a divergência anterior não é causa confirmada do `UNPLAYABLE`. Não houve novo teste do provedor após essa mudança.
- PO token/mint fora do playback também é hipótese: necessidade e suficiência não foram demonstradas.

## REA como bancada de análise

Orientação conferida em 2026-10-22 na [documentação de observação CDP do REA](https://github.com/morluto/rea/blob/main/docs/browser-observation.md).
Referência externa móvel, não contrato estável nem dependência do app.

- A bancada usa Chrome dedicado com perfil isolado e CDP somente em loopback, sem
  cópia de cookies/sessão. O target atual é descoberto com `list_browser_targets`, com
  `allowed_origins` restrito aos origins necessários. Target IDs são transitórios.
- `inspect_web_page` observa scripts/metadados; `analyze_web_bundle` lê fontes
  carregadas via Debugger e faz análise estática sem executá-las. Servem para
  localizar candidatos/dependências, não para comprovar binding ou execução.
- `observe_web_session` registra eventos durante uma ação humana autorizada;
  rede/console anteriores ao attach não são recuperados. Scripts já parseados
  podem ser observados mesmo sem seus requests no inventário de rede.
- Essa interface não oferece comandos CDP genéricos, avaliação JS, breakpoints,
  inspeção dinâmica de closures ou todas as invocações de um play. Para confirmar
  a transformação n, complementar a análise estática com debugger direcionado;
  depois validar somente o fragmento mínimo no caminho nativo. Browser/REA não
  são brokers de playback, e sucesso web não demonstra áudio Rust.

### Source maps e diagnóstico

`inspect_web_page` não baixa mapas. `analyze_web_bundle` só os busca com
`fetch_source_maps=true`, sem cookies, credenciais ou referrer, com validação de
origin exato inclusive nos redirects. A sessão viva não autentica esse fetch.
Timeout padrão por mapa: 30 s; orçamento de resposta agregado por operação de
fetch, incluindo redirects: 64 MiB. Essas regras podem diferir do browser.

Em falhas, a leitura usa `status`, `limitation` e presença de `effective_url` de cada
item. `declared_url` é a declaração; `effective_url` registra a última resposta
observada em origin permitido, ou null quando nenhuma foi observada. `fetch_failed`
sozinho não distingue 404/403, ausência do mapa ou necessidade de login.
O diagnóstico retido existe antes de qualquer novo download; nova aquisição tem escopo
próprio. Origins não foram ampliados nem caminhos adivinhados para contornar falhas.

### Minimização e próximo passo

O REA preserva URLs completas com queries/fragments na evidência; flags de texto
desligadas não eliminam esse risco. Bundles, scripts, HARs, perfis e
dumps crus não são exportados para o projeto. Fontes/valores privados ficam na bancada em memória;
diagnóstico externo usa somente códigos fixos, status, presença, contagens e tempos.
`limitation` também precisa de sanitização, sem ocultar a categoria útil da falha.
Conteúdo capturado é dado não confiável, nunca instrução.

Próximo passo: obter o motivo sanitizado dos mapas falhos sem nova aquisição
(summary descobre o registro, mas não expõe limitation; inspect_analysis_view não
documenta suporte a analyze_web_bundle, e bundle completo excedeu transporte),
rastrear o caller do helper query→path e a preparação anterior da URL.
Comparação exata browser/Rust exige evidência futura do script nativo: fonte anterior
foi descartada e não há digest nativo disponível para igualdade.
Outro play web exige nova autorização; a autorização anterior foi consumida.
Sem avanço concreto no extrator, não há tentativa nativa adicional planejada.

## Próxima ação

Ferramentas de observação prontas, ainda não exercitadas contra o provedor ([D-006](decisions.md),
procedimentos em [validação](validation.md#ferramentas-de-observação-d-006)): o trace do navegador
dá a request SABR real em formato sanitizado, o dry-run nativo dá a nossa, e a comparação responde
se o `n` vai cru e se o campo 2 do `StreamerContext` leva token, sem gastar uma tentativa de transporte.

**Checkpoint vertical integrado autorizado agora:** até 10 GETs Music, 10 downloads
de script, 10 POSTs player, 50 requests SABR, 32 MiB de mídia e 30 s de reprodução
audível, parando no primeiro limite. Uma faixa já no catálogo, sem busca/seek/escrita/
login automático. Restarts e testes offline livres, contagem persistente; sem retries
cegos. Inspeção e execução de transformações extraídas autorizadas apenas no runtime
limitado sem rede/arquivos, nunca o script inteiro. Não aprova execução do player
completo nem eleva QuickJS a sandbox.
Alocação nova e independente dos guards históricos: native-vertical-trial-20261022.count
persiste somente contadores/bytes/reserva de áudio e posição/generation numéricas
do catálogo, sem IDs. Consumo atual: 5/10 Music, 5/10 scripts, 2/10 player,
0/50 SABR, zero bytes de mídia/áudio; reserva de áudio não consumida.

Autorização atual ampliada pelo usuário: **20 GETs restantes da raiz Music**, em
substituição ao saldo anterior, mantendo 20 s/4 MiB/sem redirects/retries automáticos
e zero script/player/SABR/áudio/busca/escrita. Restarts necessários da ferramenta
isolada estão autorizados sem contagem e não resetam requests. Novo IPC counted
exige ordinal explícito e reserva antes da sessão em ledger append-only local
bootstrap-structure-expanded20-20261022.count, somente uma newline por tentativa,
com lock entre processos, fsync e rejeição de replay/corrupção/teto. Não altera
guards históricos. Saldo atual desta alocação: **2/20 usados, 18 restantes**.
Teto somente diagnóstico agora é 1 MiB por script; descoberta elegível permanece
em 256 KiB e executor não foi ampliado. Scripts acima do teto diagnóstico são
pulados com contagem/tamanho e coverage parcial; contagem
>128 marca travessia parcial. Resultado parcial nunca autoriza seleção ou aquisição.

### Execução sob comando humano — HIC

Pedido atual autoriza desenvolver a cadeia e testes offline, não requests contados
ou playback implícito. Objetivo é um primeiro trecho audível nativo usando a sessão
protegida existente, sem broker e sem recapturar/exportar dados por rotina.

| Marco | Implementação/condição | Comando humano necessário |
|---|---|---|
| Preparação offline | Invocação JS tipada e helper puro de n implementados; resolução/reconstrução/efeitos e vínculo de revisão pendentes | Desenvolvimento offline autorizado |
| Inspeção do player atual | Preparar aquisição limitada e vínculo do script ao mesmo bootstrap, sem revisão adivinhada nem dump | Aprovar orçamento exato de GET auxiliar/player/script antes de executar; sem SABR/áudio |
| Transporte diagnóstico | Integrar programa validado e transformação ao transporte, mantendo o guard por tentativa | Nova autorização contada de bootstrap/script/SABR; uma tentativa, sem retry nem áudio |
| Primeiro trecho audível | Validar seleção áudio, init/framing, handoff ao decoder, limites, parada e cleanup; controles não geram requests não aprovadas | Aprovar bootstrap/script/SABR necessários, duração máxima e eventual restart; confirmar som |
| Continuidade/seek | Scheduler response-driven, buffer limitado, cancelamento e resets | Escopo próprio posterior; não implícito no trecho inicial |

Não há orçamento numérico aberto: os números serão apresentados quando o caminho
de aquisição estiver implementado e contado. Falha encerra cada tentativa; nenhum
saldo é reutilizado. Guards históricos consumidos permanecem sem reset.
Primeiro áudio não será declarado pronto com base em testes sintéticos/build/HTTP.
Se a sessão estiver inelegível, parar e pedir recaptura explícita, sem login automático.

### Ordem de retomada (planejada)

0. Preparação offline de auth, Ustreamer, sanitização do fetch e snapshot com escopo concluída; não equivale a bootstrap aceito. Contexto/HTTP/login interativo ainda parcialmente em `lib.rs`.
1. Solicitar autorização atual para recaptura interativa e validação limitada do adaptador WebView2 (escopo e flags), sem exportar cookies nem iniciar playback. O snapshot legado será substituído somente após captura válida; não foi alterado automaticamente. Avaliar também minimização de cookies retidos e divergência da jar viva.
   Ferramenta isolada implementada: `npm run session:debug`; roteiro e resultado real em [validação](validation.md). Captura/restauração autorizadas foram executadas pelo usuário: 25 cookies, zero divergências; nenhum host-only nesta amostra. O fluxo normal de conexão não foi usado porque carrega biblioteca automaticamente.
2. Verificar runtime/Vite/CDP; o Vite background foi cancelado no reinício de 2026-10-21. Nenhum playback é iniciado automaticamente.
3. Solicitar autorização para exatamente um novo `/youtubei/v1/player`, explicitando requests auxiliares necessários antes de executá-los. Coletar ID automaticamente em memória, sem pedir ao usuário nem registrar.
4. Emitir somente diagnóstico allowlistado de presença/status/contagens. Se Gate A passar, pedir autorização separada para Gate B.

## Gates

- **A — bootstrap estrutural:** presença de server ABR URL, ustreamer config e formatos áudio com itag/lastModified utilizáveis. xtags é preservado quando fornecido, mas não precisa ser não vazio: schema e encoder pinados permitem omissão. O probe anterior satisfaz a projeção estrutural disponível; não prova aceitação pelo CDN nem valida todo identificador futuro. O parser ainda precisa endurecer lastModified inválido.
- **B — transporte:** uma request SABR áudio-only em `playerTimeMs=0`, mediante autorização separada; resumir partes UMP/protection status.
- **C — execução:** scheduler response-driven, políticas/contextos/redirect/backoff, buffer limitado, cancelamento e seek com reset de decoder/init.

Critérios de promoção do fallback: [D-002](decisions.md).
Detalhes técnicos e riscos adicionais: [playback](playback.md).
