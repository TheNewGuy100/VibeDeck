# Playback — contratos, protocolo e limites

Revisado em 2026-10-21. Interfaces internas instáveis; referências reversas não são contrato oficial. Regras: [AGENTS.md](../AGENTS.md). Navegação: [índice](README.md).

## 1. Estado atual e objetivo

| Caminho | Situação |
|---|---|
| Relay contínuo `play_youtube_audio_ump_stream` | Caminho normal atual, áudio nativo validado em 2026-10-20; WebView ainda faz entrega/scheduling |
| `play_youtube_audio_ump_first_response` | Protótipo antigo, ainda registrado, uma resposta apenas; não fornece faixa completa/seek |
| Resolver URL/GET `play_audio_url` | Caminho legado separado; som e seek confirmados em 2026-10-07, não prova seek no relay |
| Cliente SABR próprio em Rust | Encoder/parser/bootstrap experimentais; sem reprodução nativa SABR validada |
| Faixa de bytes nativa (`native-range-playback`) | Caminho principal opcional: `/player` do app, `n` e `sig` pelo player preparado, UMP por faixa de bytes sem `pot` e sem cookies na mídia; pausa, seek exato, renovação e fim validados no Windows (ver [validação](validation.md)); cai no relay se não iniciar |

**Decisão:** eliminar todos os brokers do caminho de reprodução, com scheduler SABR e seek próprios em Rust. Manter relay como fallback até validar o substituto; não promover o relay ou seek page-driven como solução final. WebView de eventual atestação, fora do playback, é hipótese a avaliar, não implementação confirmada.

## 2. Experimento nativo

- Feature debug opt-in `native-playback-trial`: bootstrap → script declarado →
  timestamp → player → transformação n extraída → SABR → AudioEngine. Ledger
  persistente limita operações/bytes e reserva áudio com watchdog de 30 s; sem
  chamada automática. QuickJS limitado executa somente fragmento extraído, não o
  player completo; não é sandbox de segurança. Envelopes s/sp e continuidade
  completa não são suportados. Tentativas reais ainda bloqueadas no reconhecedor
  de n antes do SABR; ver [validação](validation.md).

- `src-tauri/src/sabr/{mod,proto}.rs`: protobuf/base64 manuais (sem prost-build/protoc), bootstrap, request builder e parser UMP/controle. `mod sabr` sob Windows.
- `fetch_youtube_player` retorna `PlayerFetch` com payload e contexto transitório; credenciais nunca devem sair para frontend/log.
- `debug_youtube_sabr_probe` é debug-only: projeta status, categoria fixa do motivo, presença de campos e contagens, sem payload cru.
- Bootstrap e probe compartilham `player_ustreamer_config`: lê o JSON cru em `playerConfig.mediaCommonConfig.mediaUstreamerRequestConfig.videoPlaybackUstreamerConfig`, decodifica base64 e rejeita bytes vazios. Cobertura sintética; não infere config de `streamingData` nem prova bootstrap aceito.
- Estado de retomada, riscos de cookies/auth e gates: [investigação ativa](investigation.md).
- Critérios de substituição do fallback: [D-002](decisions.md).

## 3. Mapa de chamadas e autenticação

| Operação | Método/caminho | Implementação e limite |
|---|---|---|
| Bootstrap de config | GET raiz `music.youtube.com` | Extrai versão, visitor e API key; atualmente com cookie no fetch de player; parser HTML frágil |
| Busca | POST `/youtubei/v1/search` | WEB_REMIX, normaliza renderers em LibraryItem, limite atual 25; fixtures sanitizadas/paginação pendentes |
| Player | POST `/youtubei/v1/player` | Native fetch + caminho da própria página; respostas iniciais e heartbeat precisam ser distinguidos |
| Biblioteca/faixas | POST `/youtubei/v1/browse` observado no WebView | Sessão/cookies internos; não confundir com OAuth oficial |
| Continuação | POST `/youtubei/v1/browse` | Context + continuation, query ctoken/continuation/type e key opcional; valores só em memória |
| Reprodução web | `/watch`, `/player`, `/next`, `/player/heartbeat` | Página autenticada obtém contexto e agenda entregas; não é cliente nativo próprio |
| Mídia | POST HTTPS `*.googlevideo.com/videoplayback` | Resposta UMP; caminhos antigos ranged e atuais SABR têm formatos diferentes |
| Escritas em playlists | Não implementadas | Não há contrato atual de create/add/remove/delete/rename/privacy validado |

- Sessão nativa em `youtube-session.bin`, protegida via Windows DPAPI. Presença do arquivo não comprova aceitação pelo serviço. WebView tem jar viva, distinta de snapshot; restaurar snapshot não garante sincronismo.
- Persistência e hashes compartilhados pertencem a `youtube_session.rs`: cookie elegível por URL/path/instante e esquema SAPISID/1P/3P, fallback de SAPISID para 3P e sufixo `_u` somente com identidade incluída no digest. Consumidores não fornecem identidade. Snapshot v2/compatibilidade e adaptador WebView2: [arquitetura](architecture.md). Validação sintética/DPAPI local, não aceitação do provedor; sessão antiga exige recaptura.
- Google OAuth/YouTube Data API é caminho separado. Não assumir access token OAuth como autenticação das operações Innertube.
- `signatureCipher` contém URL/s/sp; decipher de s e transformação n são separados. pot/cpn/contexto/expiração podem ser necessários. Resolver direto atual só seleciona formatos com URL; decipher/n/renovação completos não estão implementados.
- Selecionar áudio por MIME/codec/qualidade/bitrate/sample rate/canais, não por itag fixo. AAC/MP4 (140/141) e Opus/WebM (249/250) são observações, não disponibilidade garantida.

## 4. Relay contínuo atual — contrato e limites

- UI (`GoogleSyncService.playAudio`) chama `play_youtube_audio_ump_stream`. Contrato: `scope=continuous_sabr_relay`, engine started, `continuation=true`, **`seek=false`**.
- Hidden `youtube-stream-bridge` navega watch e reproduz página mutada (elemento muted/volume zero + WebView2 SetIsMuted). YouTube ainda controla requests/ritmo; output audível é Symphonia/CPAL nativo.
- WebResourceResponseReceived aceita POST HTTPS googlevideo `/videoplayback`, 2xx, MIME UMP; não exige markers query antigos. GetContent/IStream pode retornar sem corpo: candidato sem stream é descartado e espera próximo dentro do deadline (~15 s).
- Continuous capture usa canal limitado de 4 slots e send com timeout (poll 10 ms); handoff engine de 8 slots. Canal limitado não torna o buffer da faixa limitado.
- `ActiveUmpRelay` é dono da sessão/ativação; substituição cancela anterior e espera cleanup até 5 s. Guard remove handler/fecha WebView. Pausa nativa é refletida na página; ended/error/cancel/engine inativo/canal fechado encerram relay.
- Parser aceita respostas controle-only completas entre mídia; finish final exige mídia global. Não presumir que qualquer parte incompleta entre HTTP responses já seja suportada.
- **Runtime 2026-10-20:** áudio audível confirmado; ativação correlacionada, decoder/output iniciados, vídeo com zero bytes emitidos. Múltiplas respostas continuaram até fim de faixa (~231 s), fila avançou automaticamente e próxima tocou. Pausa congelou response/media bytes por pelo menos 8 s.
- Não validados nessa execução: seek, retomada após pausa prolongada, prefetch, rapid-skip robusto, todas as misturas/ordens/erros. Buffer growing permanece ilimitado durante faixa; diagnóstico pode ficar `ump_parsing/in_progress` pausado.

## 5. UMP, montagem e probe — invariantes

- Outer framing UMP usa varint próprio; protobuf dos headers usa base-128 separado e limitado. Reusar leitor UMP no protobuf causou `Truncated UMP media header protobuf` e foi corrigido.
- Partes centrais: MEDIA_HEADER 20, MEDIA 21, MEDIA_END 22, FORMAT_INITIALIZATION_METADATA 42. MEDIA começa com um byte de header ID, removido antes do demuxer.
- MediaHeader: 1 header_id, 3 itag, 6 segment_byte_range_start, 8 initialization flag, 9 segment_num, 14 segment_length_bytes; campos 11/12 também fazem parte da referência SABR revisada.
- Selecionar primeiro formato **áudio** declarado por part 42; se metadata já apareceu, não emitir vídeo enquanto áudio ainda não chegou. Sem metadata, fallback legado só ao header mais recente.
- Cursor por header (`UmpHeaderState`), não global; segmentos intercalam. Decidir duplicate uma vez no primeiro chunk do header; descartar init/segmento/range já emitidos sem perder chunks novos do mesmo segmento.
- `decode_media` prefere track com parâmetros de áudio antes de default_track. Vídeo entrando primeiro no stream causava seleção H.264/codec_error; teste reproduz ordem vídeo→áudio e runtime posterior não repetiu erro.
- `GrowingReader` vivo sem tamanho total **não** se declara seekable; só com Content-Length conhecido ou produtor finalizado. Antes retornava true com byte_len None e Symphonia ISOM4 falhava com SeekError(Unseekable). Fix validado com áudio em runtime.
- Classificar box BMFF pelos bytes 4..8 após size, aguardando oito bytes mesmo divididos entre MEDIA. Offset zero produzia diagnóstico unknown falso.
- Diagnóstico limitado a 16 segmentos/8 formatos e partes metadata de até 64 KiB; expõe ordem/header/itag/init/segment/start/length/media/emitted bytes/end, MIME/box em categorias fixas. Sem payload armazenado.
- `get_audio_playback_diagnostic`: read-only, allowlist de correlação/etapa/status/contagens/milestones/errorCode/probeErrorKind/umpSegments/umpFormats. Comparar sequência com ativação atual; idle ou mismatch não diagnostica tentativa descartada.
- Fixtures cobrem framing dividido/varints/truncamento/filter/dedup/ordem/controle-only; WAV PCM sintético faz UMP → GrowingReader → probe. Fixture válida não prova conteúdo YouTube real.

## 6. SABR próprio — referência, não contrato de servidor

Fonte pinada: [LuanRT/googlevideo@96d1418984d00ac2797a1dafe4625d1368b5b9aa](https://github.com/LuanRT/googlevideo/tree/96d1418984d00ac2797a1dafe4625d1368b5b9aa).
Arquivos: SabrStream, SabrSegmentBuffer, SabrStreamingAdapter, SabrUmpProcessor e protos video_playback_abr_request, buffered_range, media_header, next_request_policy, playback_cookie. Outra referência: [gsuberland/UMP_Format](https://github.com/gsuberland/UMP_Format/blob/main/UMP_Format.md).

- Modelo da referência: bootstrap → POST protobuf VideoPlaybackAbrRequest (ClientAbrState/player time/buffer/formatos/contexto) → processar mídia e controles → atualizar contexto/policy/redirect/reload/backoff → próxima request.
- Schema revisado: ClientAbrState.player_time_ms campo 28, StreamerContext, controle UMP 35/43/44/45/46/57/58/59/62. Referência usa rn crescente, alr=no, application/x-protobuf, accept UMP, accept-encoding identity.
- Progresso segue estado temporal/buffer/policy, não incremento isolado de range. Seek backward na referência reseta init-format state; implementar reset próprio com teste, não supor equivalência com browser.
- Browser observado em 2026-10-18: 34 POST deliveries (33 concluídas 200 UMP); query-name sabr/cpn/rn, sem range/ump/mime/pot/rbuf; corpos opacos. Um seek para 120 s retomou perto de 129 s com buffer separado [119,8–149,7], sem preencher o gap anterior. Sugere demanda perto do destino, não demonstra algoritmo universal.
- Não confundir parser/request builder local com scheduler completo nem terceiro com evidência de aceitação WEB_REMIX atual.

### Identificador de formato — revisão local em 2026-10-21

- `FormatId::encode` escreve itag no campo 1, lastModified no 2 e xtags no 3.
  `Writer::string_field` omite strings vazias; portanto xtags ausente no JSON
  resulta em **campo 3 ausente**, não em uma string vazia explicitamente enviada.
  xtags não vazio é preservado pelo bootstrap e enviado como UTF-8, sem derivação
  de signatureCipher, URL ou itag. Ausente, vazio e tipo JSON inválido atualmente
  colapsam no mesmo estado; o tipo Rust não preserva presença.
- `lastModified` é lido como u64 numérico/string; ausente ou inválido vira zero,
  também omitido pelo Writer. O parser aceitar bootstrap não garante identificador
  completo. A amostra real anterior tinha esse campo nos quatro formatos, mas isso
  não cobre a validação de entradas futuras.
- `build_abr_request` reutiliza o mesmo identificador selecionado nos campos 2 e
  16; buffered ranges usam o encoder de FormatId. Os testes existentes usam fixture
  sem xtags, mas não verificam explicitamente os bytes do identificador, presença
  versus vazio, xtags não vazio ou rejeição de lastModified inválido. O teste de
  request cobre player time não zero, não a omissão do campo 28 quando vale zero.
- Conclusão limitada à leitura local: a ausência de xtags não é erro de protobuf
  por si só; opcionalidade semântica e compatibilidade com a referência pinada
  ainda precisam de conferência do schema/encoder upstream. Nenhuma cópia local
  desses arquivos foi localizada nos caminhos pesquisados; não houve acesso à rede.
  Essa foi a conclusão da leitura estritamente local; a conferência pública abaixo
  resolve a opcionalidade sem demonstrar aceitação pelo servidor.

### Conferência pública pinada de FormatId (2026-10-21)

Fontes do mesmo commit `96d1418984d00ac2797a1dafe4625d1368b5b9aa`:
[schema](https://github.com/LuanRT/googlevideo/blob/96d1418984d00ac2797a1dafe4625d1368b5b9aa/protos/misc/common.proto),
[encoder gerado](https://github.com/LuanRT/googlevideo/blob/96d1418984d00ac2797a1dafe4625d1368b5b9aa/protos/generated/misc/common.ts),
[formatUtils](https://github.com/LuanRT/googlevideo/blob/96d1418984d00ac2797a1dafe4625d1368b5b9aa/src/utils/formatUtils.ts),
[SabrStream](https://github.com/LuanRT/googlevideo/blob/96d1418984d00ac2797a1dafe4625d1368b5b9aa/src/core/SabrStream.ts),
[TrackCollection](https://github.com/LuanRT/googlevideo/blob/96d1418984d00ac2797a1dafe4625d1368b5b9aa/src/utils/TrackCollection.ts) e
[request schema](https://github.com/LuanRT/googlevideo/blob/96d1418984d00ac2797a1dafe4625d1368b5b9aa/protos/video_streaming/video_playback_abr_request.proto).

- `FormatId` é proto2 com três campos opcionais. O encoder gerado omite xtags
  undefined/vazio e lastModified undefined/"0". Para identificadores válidos com
  xtags ausente/vazio, a omissão local coincide com esse encoder por inspeção;
  não houve execução diferencial dos dois encoders.
- `buildSabrFormat` passa xtags sem fabricar valor; `createFormatKey` usa
  `itag:xtags` e normaliza ausência/vazio com `|| ''`. Não há exigência de xtags
  não vazio na seleção da referência. Portanto Gate A não deve exigir esse valor;
  preservar quando fornecido e omitir quando ausente. Não derivar de cipher/URL.
- Isso não prova equivalência de toda a request, nem aceitação WEB_REMIX atual.
  A factory usa decipher na server ABR URL e callbacks de PO token/reload; essas
  diferenças continuam fora da conclusão sobre FormatId.
- Divergência separada: campo 2 da request é `initialization_format_ids`, não a
  seleção preferida. A referência o obtém dos tracks inicializados, inicialmente
  vazios numa sessão nova sem snapshot. Rust envia o áudio selecionado no campo 2
  incondicionalmente, além do campo 16 correto de seleção. Corrigir/modelar esse
  estado e testar a primeira request antes de Gate B; a causa/efeito no servidor
  não foi medida. Não remover o campo de todas as requests de uma futura sessão.
- Correção offline posterior: `AbrRequestState.initialized_formats` começa vazio;
  campo 2 serializa somente essa lista, independente da seleção do campo 16.
  O coordenador futuro deve preenchê-la após confirmação por metadata e limpá-la
  no reset de inicialização. Não há scheduler atual fazendo essa atualização.
- Bootstrap agora descarta formatos com itag não positivo ou lastModified
  ausente/inválido/zero; se nenhum áudio utilizável restar, retorna o código
  existente `missing_audio_formats`. lastModified positivo é uma política local
  conservadora, não uma obrigatoriedade do schema proto2.
- Testes sintéticos cobrem bytes exatos de FormatId vazio/sem xtags/com UTF-8,
  roundtrip, campo 2 vazio→confirmado→reset, seleção áudio preservada, omissão de
  playerTimeMs zero, lastModified inválido e fallback para outro áudio utilizável.
  Não validam confirmação automática por metadata nem transporte.

### Preparação do Gate B (2026-10-21)

`sabr/transport_probe.rs` é debug-only. O comando explícito
`debug_youtube_sabr_transport_once` está registrado apenas no handler Windows
debug; nenhuma UI/startup o invoca. Aceita somente janela main e orçamento exato
`{auxiliaryGet:1,playerPost:1,sabrPost:1}`. Reserva uma tentativa por processo
antes de ler catálogo/sessão; falhas também consomem a tentativa, sem reset IPC.
Escolhe youtubeId válido do catálogo local só em memória, faz GET/player novos
e no máximo um SABR. Orçamento é um limite declarado, não autorização automática.
`run_once` consome bootstrap em memória e prepara uma primeira request áudio-only
em tempo zero, com rn=0/alr=no. Não recebe cookies/Authorization da sessão.
Rejeita URL fora de HTTPS/subdomínio googlevideo/porta 443/path videoplayback,
userinfo e fragment; presença de s/n bloqueia com código fixo, sem tentar decipher.
Timeout total de request: 20 s; request até 256 KiB, resposta até 4 MiB e 4096
partes. Redirects e retry do reqwest explicitamente desabilitados. Exige MIME UMP
e encoding ausente/identity; não segue controles redirect/reload nem inicia áudio.
Resumo valida framing completo, mas não toda a semântica dos headers/metadata.
Proteção exporta somente valores numéricos 0–3, sem atribuir significado, e flag
para outros valores; controles e partes desconhecidas viram presença/contagens.
Bytes de MEDIA incluem o byte de header ID e não demonstram áudio decodificável.

Evolução do probe em 2026-10-22 ([D-006](decisions.md)): o comando aceita `allowRawN`
(envia o `n` como recebido; `s` continua bloqueando) e `dryRun` (monta a request exata e
projeta seu formato sem POST SABR; orçamento `{auxiliaryGet:1,playerPost:1,sabrPost:0}`,
ainda faz o GET e o player). Depois do bootstrap o comando devolve sempre um relatório:
o primeiro erro vira `errorCode` e ficam `urlShape`/`requestShape` (nomes e classes, nunca
valores), `statusClass`, `contentTypeClass`, `responseHeaderNames`, `sent` e `dryRun`. Só as
falhas anteriores ao bootstrap continuam como erro do IPC. Formato em
`sabr/request_shape.rs`, com a mesma numeração e classes do trace de `scripts/sabr-trace`.

Revisão estática concluída em 2026-10-21 contra
[factory pinada](https://github.com/LuanRT/googlevideo/blob/96d1418984d00ac2797a1dafe4625d1368b5b9aa/examples/downloader/utils/sabr-stream-factory.ts),
[StreamerContext](https://github.com/LuanRT/googlevideo/blob/96d1418984d00ac2797a1dafe4625d1368b5b9aa/protos/video_streaming/streamer_context.proto)
e SabrStream da mesma revisão:

- ClientInfo tem campos opcionais; factory passa somente clientName/clientVersion.
  Rust agora omite locale/OS fixos, mantendo WEB_REMIX e versão do bootstrap.
  A factory usa player WEB, não prova equivalência ao WEB_REMIX autenticado.
- Factory passa a ABR URL por decipher. Rust não implementa transformação e bloqueia
  s/n, inclusive nomes codificados. Ausência desses parâmetros não prova ausência
  de qualquer outra transformação necessária; não alegar equivalência ao decipher.
- makeStreamingRequest muda rn; setStreamingURL muda alr. Não geram cpn nesse
  caminho. Rust preserva eventual cpn da URL, sem fabricar ou inferir obrigatoriedade.
- PO token é opcional na configuração de SabrStream; factory instala um minter.
  SabrStream tenta mint no início sem aguardá-lo e reage a status 2/3 com nova
  tentativa. Rust só usa token recebido no bootstrap; ausência não prova que seja
  dispensável. Gate B pode terminar com proteção pendente/rejeitada e zero mídia.
- Diferenças deliberadas: sem mint, heartbeat, reload, fallback, redirect ou retry;
  timeout absoluto menor e corpo limitado. Contextos/policy recebidos não geram
  outra request. ClientAbrState da referência também informa capacidades/track/DRC;
  os campos não modelados não devem ser copiados por tentativa cega.

Query agora preserva os pares crus e sua ordem/codificação, alterando somente
rn/alr; se sparams/lsparams declarar esses campos assinados, bloqueia. Isso tem
cobertura sintética, não validação no CDN. As outras diferenças continuam abertas.
O cliente HTTP compartilhado do bootstrap também desabilita retry explicitamente.
O comando sempre obtém bootstrap novo dentro do orçamento; não renova nem repete
após erro de player/CDN. Revisão permite propor um probe **diagnóstico**, condicionado
aos bloqueios locais, não promover o cliente a playback. Execução requer autorização
contada separada; resultados negativos não isolam auth, token, contexto ou decipher.

### Revisão do decipher e contrato planejado (2026-10-21)

Leitura pública pinada de YouTube.js
[Player.ts](https://github.com/LuanRT/YouTube.js/blob/78375cbf808544d91b0d5df99529a20f4a5a18eb/src/core/Player.ts),
[matchers.ts](https://github.com/LuanRT/YouTube.js/blob/78375cbf808544d91b0d5df99529a20f4a5a18eb/src/utils/javascript/matchers.ts),
[JsExtractor.ts](https://github.com/LuanRT/YouTube.js/blob/78375cbf808544d91b0d5df99529a20f4a5a18eb/src/utils/javascript/JsExtractor.ts)
e [Utils.ts](https://github.com/LuanRT/YouTube.js/blob/78375cbf808544d91b0d5df99529a20f4a5a18eb/src/utils/Utils.ts).
Esse commit já era referência do parser; não foi conferido como versão exata da
dependência instalada pela factory googlevideo. Não é prova do player atual.

**Constatações por leitura, não execução:**

- Player.create obtém identidade do player via iframe_api quando não fornecida,
  baixa base.js e extrai AST/dependências com JsAnalyzer/JsExtractor. Portanto um
  futuro fetch de script acrescenta requests ao orçamento; não cabe implicitamente
  no GET auxiliar já autorizado e consumido. Não chamar create por adaptação cega.
- O matcher procura um construtor de URL com parâmetros e atribuição alr=yes,
  não uma lista estável de reverse/swap/splice. A extração remove alguns
  inicializadores com efeitos, mas isso não é sandbox nem limite de execução.
- getNsigProcessorFn executa o construtor extraído sobre uma URL sintética e chama
  métodos do protótipo fora de uma blacklist. O host usa Platform.shim.eval;
  o resultado não pode ser substituído por uma transformação fixa sem evidência.
- decipher lê n da URL; s/sp vêm do envelope signatureCipher/cipher. Passar uma
  ABR URL direta não equivale a tratar s nela como envelope. O guard local de s
  permanece conservador; não está demonstrado que o caso real tenha s, n ou ambos.
- O helper também altera cver por identidade do cliente e injeta pot apenas fora
  de sabr=1. Não copiar versões fixas nem token para query. São diferenças adicionais
  ao fluxo Rust que preserva pares, não uma explicação isolada do bloqueio real.
- A referência aceita strings e pode conservar resultado enhanced_except_ com aviso.
  A implementação planejada deve rejeitar erro, vazio, tipo inválido e saída acima
  do limite; não reproduzir logs upstream de assinatura, n ou URL.

**Contrato planejado, ainda sem implementação ou dependência nova:**

#### Origem da revisão: reconferência pública em 2026-10-22

Leitura de Player.ts acima, [Session.ts](https://github.com/LuanRT/YouTube.js/blob/78375cbf808544d91b0d5df99529a20f4a5a18eb/src/core/Session.ts)
e [Innertube.ts](https://github.com/LuanRT/YouTube.js/blob/78375cbf808544d91b0d5df99529a20f4a5a18eb/src/Innertube.ts),
todos na mesma revisão pinada, confirma este caminho (não executado):

- Session.create passa options.player_id a Player.create, sem derivá-lo do config
  da sessão. Sem override, Player.create faz GET /iframe_api e extrai a revisão do
  texto entre os delimitadores escapados player/ e /. Não executa iframe_api.
- Sem cache utilizável, constrói /s/player/{revisão}/player_es6.vflset/en_US/base.js
  e baixa a fonte. Esse caminho é construção explícita do upstream, não declaração
  observada no bootstrap Music. O getter Player.url usa player_ias, diferente do
  caminho efetivamente baixado por create; não confundir esses dois usos.
- O timestamp é extraído da fonte junto da função nsig. getBasicInfo/getInfo enviam
  session.player.signature_timestamp em playbackContext.contentPlaybackContext.
  Isso associa o pedido ao timestamp escolhido pelo cliente, mas não comprova que
  a resposta declara/atesta a mesma revisão nem compatibilidade WEB_REMIX atual.
- Esse fluxo não sustenta substituir assets.js por PLAYER_JS_URL. Tampouco sustenta
  chamar iframe_api automaticamente ou aceitar uma revisão histórica como fallback.
  O resolver local posterior ao player não reproduz a ordem do upstream.

Próxima escolha: observar declaração de script no mesmo GET Music com diagnóstico
limitado, ou implementar aquisição pré-player via iframe_api com orçamento adicional
e validação própria. Não misturar silenciosamente as origens. Nenhum desses caminhos
foi integrado/validado por esta leitura; nenhum request ao YouTube foi feito.

Diagnóstico debug-only preparado em sabr/bootstrap_probe.rs: IPC
debug_youtube_bootstrap_once, janela main, orçamento exato {auxiliaryGet:1}, campos
extras rejeitados e tentativa atômica antes de ler sessão. Um GET raiz Music com
snapshot protegido elegível; cliente compartilhado sem redirects/retries e timeout
20 s, MIME text/html, UTF-8 e corpo até 4 MiB. Sem catálogo/player/script/SABR/áudio.
Retorno é somente projeção lexical: quatro chaves fixas PLAYER_JS_URL/jsUrl/js/STS,
contagens/tipos iniciais, quantidade de caminhos candidatos distintos e marcadores
crus/escapados. Contagens saturam em 32, com flag de truncamento. Valores transitam
somente em memória, não são retornados nem logados. Regex aceita falsos positivos
em comentários/strings/HTML inerte e não cobre todas as formas JS/HTML; coverage
lexical_only é explícita. Não seleciona scripts, não confirma declarações e não
constitui binding nem autorização de aquisição. O campo STS é pista, não timestamp
validado. Fixtures exclusivamente sintéticas cobrem limites, projeção e orçamento.

Etapa estrutural isolada: classify_js_url_properties em tools/decipher-eval/src/bootstrap.rs
parseia uma fonte JavaScript selecionada pelo chamador (até 256 KiB) e retorna somente
contagens de propriedades jsUrl por sintaxe. Comentários/strings/regex não viram
propriedades; escapes no nome são decodificados pelo parser. Computed literal,
accessor/método e valores não string são separados. Inclui contextos condicionais,
aninhados e mortos: não prova execução, unicidade elegível, ownership ou vínculo.
Não resolve aliases, assignments ou chaves computadas dinâmicas; zero não prova
ausência. Não é parser HTML, não seleciona scripts, não executa JS e não integra
aquisição. Limites de fonte não são teto absoluto de recursos do Oxc.

Seleção HTML experimental separada em bootstrap_html.rs usa html5ever/rcdom 0.38.0
(somente crate isolada). Parse HTML com scripting enabled e rejeição de erros;
percorre DOM HTML excluindo template/noscript e subárvores foreign. Analisa somente
scripts inline classic no subconjunto type ausente/vazio/text/javascript/application/javascript,
language ausente/vazio/javascript e sem nomodule; src presente, mesmo vazio, é
externo e nunca adquirido. JSON/module/tipos não suportados são contados, não
executados. Texto cru de script não decodifica entidades HTML. Duplicatas/falhas JS
continuam contagens, sem seleção ou autorização. Limites: HTML 4 MiB, script 256 KiB,
128 scripts percorridos, 100000 nós visitados. Limites de travessia são posteriores
ao parse e não abrangem nós de subárvores excluídas: não são teto de RSS/CPU ou de
teardown da árvore. Implementação não emula browser, CSP ou execução/ordem; ausência
nesse subconjunto não prova ausência no documento/runtime. Ainda sem contexto
config→player, integração no app, aquisição ou decipher.

1. Resolver de script separado de sessão, transporte SABR e decoder. Identidade,
   fonte e resultado transitórios em memória, sem Debug/IPC/dump de valores.
   Vincular programa à identidade do player obtida no mesmo bootstrap; não usar
   programa de revisão diferente como fallback. Falta definir origem desse vínculo.
2. Parser/extrator estrutural com limites de fonte, nós e dependências. Formato
   desconhecido/ambíguo falha fechado; regex isolada não comprova extração correta.
3. Avaliar executor embutido acessível por Rust, sem WebView/Node/sidecar, sem host
   bindings para rede, disco, ambiente ou credenciais. Exigir limites efetivos de
   memória, instruções/interrupção, pilha e tamanho de saída. Timeout async externo
   sozinho não interrompe JS síncrono. Engine e custo ainda não foram escolhidos.
4. Transformar somente valor de desafio suportado; preservar bytes e ordem dos
   demais pares. Rejeitar duplicatas/nomes ambíguos, conflito com parâmetros assinados
   e envelope não suportado. Não transformar assinatura sobre dados já modificados.
   A URL resultante deve passar novamente pela validação de destino antes do HTTP.
5. Fixtures exclusivamente sintéticas: extração única/ausente/ambígua e dependências;
   transformação determinística; n separado de s/sp; percent-encoding e pares
   intocados; erro/vazio/oversize; loop infinito/recursão/alocação; tentativa de
   acesso a host; revisão de player incompatível; nenhuma chamada HTTP/IPC/áudio.
   Testes diferenciais com uma referência pinada seriam evidência adicional, não
   substitutos da validação real. Esses testes ainda não foram implementados.

Avaliação inicial de executor em 2026-10-21:

- [rquickjs 0.14.0 Runtime](https://docs.rs/rquickjs/0.14.0/rquickjs/struct.Runtime.html)
  oferece memory_limit, max_stack_size e interrupt_handler. A documentação avisa
  que memory_limit não atua com allocator customizado/rust-alloc; essas features
  não podem ser ativadas silenciosamente. Interrupção é cooperativa na engine,
  não garantia de deadline absoluto durante toda operação nativa/compilação.
- [Boa 0.21 RuntimeLimits](https://docs.rs/boa_engine/0.21.0/boa_engine/vm/struct.RuntimeLimits.html)
  documenta limites de loops, recursão e pilha; essa API consultada não estabelece
  um teto global de heap. Não houve benchmark nem descarte definitivo da engine.
- [Oxc parser 0.95](https://docs.rs/oxc_parser/0.95.0/oxc_parser/) foi usado no primeiro reconhecedor AST;
  análise de escopo/símbolos é separada do parser. Arena/limite de fonte não provam
  teto de memória do parsing. Compilado/testado somente na crate experimental,
  sem seleção para produção ou integração ao app.

Protótipo temporário rquickjs (fora do repositório e do app) compilou em Windows
com versão exata 0.14.0, default-features=false e somente std. Cargo tree confirmou
ausência de rust-alloc/allocator, loaders e bindings de host opcionais. Cada caso
criou runtime/contexto novos: heap configurado em 8 MiB, stack 128 KiB e interrupt
por deadline de 100 ms. Transformação sintética retornou resultado esperado; fetch,
process, require, std e os estavam undefined; loop infinito, crescimento de
ArrayBuffer, recursão e throw retornaram erro. Runtime novo funcionou após falhas.
Erros foram colapsados sem mensagem: não distingue formalmente OOM/stack/interrupt.
Tempos e limites de evidência em [validação](validation.md). Isso recomenda rquickjs
para o próximo spike, **não o aprova como sandbox de segurança** nem mede RSS/CPU
do app. Engine embutida compartilha processo e riscos nativos; ausência de bindings
não isola vulnerabilidades. Nenhuma dependência foi integrada ao VibeDeck.

Experimento durável agora em [tools/decipher-eval](../tools/decipher-eval/Cargo.toml),
crate independente com lockfile próprio, não referenciada pelo Cargo do app.
`evaluate_synthetic` aceita somente fonte transitória e cria runtime novo por chamada;
limita fonte a 256 KiB, heap da engine a 8 MiB, pilha a 128 KiB, saída UTF-8 a
16 KiB e deadline cooperativo a 100 ms. Confere tamanho na conversão engine-owned
antes da cópia para Rust; não faz coerção de objetos, pump de jobs ou logging.
Erros são categorias fixas; vazio e prefixo enhanced_except_ são rejeitados.
Esses limites são do experimento, não valores aprovados para player real.
Onze testes sintéticos cobrem interrupção inclusive envolta em try/catch (wrapper
rejeita resultado após deadline), tipos inválidos/Promise, Unicode, limites de bytes,
ausência de oito nomes de host, globais isolados, erros e alocação/recursão.
Falhas de alocação/recursão retornam Execution, sem mensagem ou atribuição exata.
Não há cancelamento externo, limite absoluto de processo, extração ou transformador
de URL implementados. Dependência nativa e superfície da engine exigem revisão
própria antes de aceitar script remoto. Fonte sintética não prova segurança geral.

Reconhecimento estrutural inicial implementado em
[extraction.rs](../tools/decipher-eval/src/extraction.rs), com Oxc 0.95.0 pinado e
lockfile atualizado apenas na crate experimental. Recebe script de até 256 KiB,
rejeita qualquer erro de parse e procura VariableDeclarator com FunctionExpression
síncrona/não-generator, binding simples, exatamente três parâmetros: identificador
e dois defaults com bindings simples, sem rest. No corpo direto exige atribuição
do primeiro parâmetro via new de um membro e chamada estática .set('alr','yes').
É um subconjunto mais restrito que o matcher pinado (que aceita três ou mais
parâmetros e não exige o nome set). Não é prova de semântica ou segurança: os dois
markers podem inclusive operar sobre objetos diferentes. Não promover por match.

Zero candidatas retorna Missing, múltiplas Ambiguous, trecho acima de 64 KiB
CandidateLimit. Candidate não implementa Debug/Serialize e mantém somente slice
exato da função original, preservando offsets em bytes. Não resolve referências,
não inclui inicializadores/dependências e não chama o executor. Reconhecer função
dentro de um IIFE não autoriza executar esse IIFE nem garante que suas referências
sejam recuperáveis. Limites de heap/deadline da engine **não** abrangem Oxc/visitor;
limitar fonte não fornece teto absoluto de memória, profundidade ou tempo de parse.
Somente entrada sintética até resolver essas limitações.

Nove testes de reconhecimento cobrem candidata única, declaração aninhada sem
execução, falsos marcadores em strings/comentários/regex, ausência/ambiguidade,
parse parcial rejeitado, formas não suportadas, requisitos de statements diretos,
limites de fonte/trecho e prefixo Unicode com offsets corretos. Total experimental:
20 testes, sem conteúdo real de player. Não houve execução diferencial upstream.

Resolução lexical conservadora implementada em
[resolution.rs](../tools/decipher-eval/src/resolution.rs), com oxc_semantic 0.95.0.
Resolve identidade dos bindings (incluindo sombreamento), percorre dependências
transitivas e devolve slices originais em ordem dependência-primeiro, sem duplicatas.
Não monta programa: os slices conservam escopos distintos e **não podem ser
concatenados num escopo único**. Não executa initializers ou contêineres.
Somente bindings externos de variáveis simples com função ou literal primitivo
são aceitos; rejeita globals não resolvidos, capturas de parâmetros externos,
aliases, objetos/getters, chamadas inicializadoras, redeclarações e bindings escritos.
Rejeita this/super, eval, with, import dinâmico e meta-properties nos trechos visitados.
DFS explícita rejeita ciclos e limita o inventário a 64 regiões incluindo a candidata.
Isso não é análise geral de efeitos: mutações por propriedades/aliases e efeitos de
código fora dos trechos não são demonstrados; ordem temporal/TDZ e identidade do
player também não são validadas. O resultado não é autorização para execução.
Os limites de recursos do QuickJS continuam sem abranger parser/análise semântica.

Plano de reconstrução em [reconstruction.rs](../tools/decipher-eval/src/reconstruction.rs):
agrupa declarações por identidade do escopo do binding, registra ancestralidade
lexical como ranges da fonte e mantém ordem original por grupo, não ordem de
dependências. Inclui declaração da entrada e slices exatos, sem gerar wrappers ou
copiar/executar statements do contêiner. Nomes iguais em escopos distintos continuam
separados. Ranges só têm significado naquela fonte; não são identidade persistente.
É planejamento, não reconstrução executável: strictness, condições de execução,
hoisting/TDZ, efeitos externos e ambiente dos contêineres não são reconstituídos.
Reparse/análise repetidos continuam sujeitos aos limites do parser, não do QuickJS.

Revisão estática da fronteira de reconstrução em 2026-10-22 (sem execução):

- O slice de VariableDeclarator não inclui `var`/`let`/`const`. O escopo do binding
  também não identifica o local de execução: `if(false){var h=function(){}}` tem
  binding no escopo externo, mas inicializador condicional. O plano atual não
  preserva esse caminho nem distingue inicialização de criação do binding.
- Criar FunctionExpression não executa seu corpo ou defaults; invocá-la pode fazê-lo.
  A aceitação do inicializador não prova pureza da função. Defaults da candidata
  também podem chamar código, mesmo quando suas referências são resolvidas.
- Escrita no binding não cobre `h.URL=...`, alterações de protótipo ou escape de
  `h` para uma chamada externa. O inventário atual pode aceitar esses casos porque
  não analisa todos os usos/efeitos do contêiner. Omissão desses statements pode
  mudar o resultado; executá-los indiscriminadamente também não é permitido.
- A entrada não recebe automaticamente as mesmas verificações de escrita e
  redeclaração aplicadas às dependências externas. `eval`/`with` são rejeitados nos
  trechos visitados, não demonstradamente em todo ambiente ancestral relevante.
- Diretivas herdadas de strictness não constam do plano. Injetar `use strict` no
  corpo da candidata, que tem parâmetros default, pode inclusive gerar SyntaxError;
  strictness precisa ser preservada no ambiente correto, não por prefixo genérico.

Próximo gate proposto: manter o plano como inventário e criar validação separada,
sem ampliá-lo diretamente para programa executável. Primeiro subconjunto deve
exigir declarações diretas em contêineres explicitamente suportados; conservar
kind da declaração, local sintático, flags/diretivas e caminho ancestral; rejeitar
controle de fluxo, ambientes dinâmicos e efeitos/escapes não classificados.
Validar entrada e dependências, distinguindo criação, inicialização e chamada.
Depois testar fixtures negativas de var condicional, TDZ, defaults com chamada,
mutação de propriedade, escape, entrada reatribuída e strictness herdada, antes
de qualquer geração. Isso é proposta de validação, ainda não implementada.

Primeira etapa estrutural implementada em
[structure.rs](../tools/decipher-eval/src/structure.rs):
`check_reconstruction_structure` exige plano lexical válido e programa composto
apenas por declarações const diretas, bindings simples e inicializadores de função
ou literal primitivo. Diretivas originais são toleradas, não copiadas nem injetadas.
Rejeita outros statements, inclusive contêineres, escritas e chamadas no nível raiz,
e verifica também declarações não selecionadas pelo inventário. É deliberadamente
mais restrito que o plano; não amplia o resolver nem gera código.
Sucesso retorna somente `()`, não um programa aprovado: corpos/defaults de funções,
efeitos diferidos, escapes internos, TDZ e ambiente do runtime continuam sem prova.
Por exemplo, uma função com throw pode passar estruturalmente sem ser executada.
Somente fixtures sintéticas; nenhuma integração com evaluator ou app.

### Análise de inicialização e efeitos pendentes (2026-10-22)

Conclusões por leitura de structure/resolution/reconstruction e do executor, não
por execução de um programa reconstruído:

1. No subconjunto estrutural atual, inicializadores de raiz são literais primitivos
   ou FunctionExpression: criar a função não executa corpo/defaults. Se a geração
   futura mantiver todas essas declarações na ordem original e chamar a entrada
   somente depois, os bindings const da raiz estarão inicializados antes da chamada.
   Isso não prova TDZ de locais internos, defaults ou execução de corpos. Ainda não
   existe gerador que imponha essa condição; ordem de dependências não a substitui.
2. Os positivos atuais com `h=function(){}` e `new h.URL(...)` não preparam uma
   propriedade URL em h. Não são fixtures funcionais de decipher. Uma simples
   concatenação seguida de chamada não constitui próximo passo válido. É preciso
   uma fixture sintética coerente com construção e métodos efetivos, e classificá-la
   antes de decidir ampliar objetos, prototypes ou statements de setup.
3. Efeitos devem ser separados por fase: criação do ambiente, defaults na chamada,
   corpo da entrada e chamadas transitivas. Ler propriedade pode executar getter;
   propriedade computada pode executar a expressão da chave; new/call executam código;
   coerção e operadores sobre objetos também podem executar código. Ausência de
   escrita no binding não demonstra ausência desses efeitos.
4. Não basta proibir toda atribuição: a candidata reconhecida já reatribui o parâmetro
   de URL. Distinguir rebind local de escrita em objeto; esta exige conhecer origem,
   ownership e possíveis aliases. Parâmetro recebido não é automaticamente objeto
   local isolado. Escape inclui retorno, armazenamento e passagem para outra função.
5. O executor usa Context::full: falta de bindings de host não significa ausência
   de intrinsics JavaScript ou acesso indireto a funções por propriedades. O resolver
   rejeitar nomes globais não é prova de confinamento. Nenhum classificador estático
   futuro deve promover esse runtime a sandbox de segurança.

Ordem de implementação proposta: fixture funcional sintética mínima → inventário
de efeitos por fase e identidade lexical (incluindo chamadas/propriedades desconhecidas
como bloqueio explícito) → classificação de defaults/locais e aliases → teste das
condições de inicialização → geração preservando diretivas/kind/ordem → comparação
offline da saída e dos erros no original sintético e no reconstruído. Comparações
devem usar runtimes novos, argumentos explícitos e verificar ausência de ações de
setup omitidas; não executar código remoto para descobrir efeitos. A API tipada
implementada posteriormente está descrita abaixo; não interpolar desafio em código-fonte.
Mesmo equivalência sintética não prova compatibilidade com player real. Vínculo à
revisão do bootstrap, limites do parser e revisão de segurança seguem gates separados.

Fixture funcional mínima adicionada aos testes de structure.rs: candidata com stub
local de URL cujo construtor retorna objeto explicitamente, método set e transformação
de string determinística. O original sintético passa estrutura e executa no runtime
limitado com argumentos fixos, incluindo defaults omitidos. O stub não implementa
semântica de URL; reverse é somente oráculo sintético, não algoritmo do provedor.
Segundo teste mostra default com IIFE/throw: passa estrutura, falha ao omitir argumento
e retorna normalmente ao fornecê-lo. Isso demonstra uma lacuna do gate, não sua correção.
Nenhum programa reconstruído é gerado/executado; efeitos do objeto local, chamadas,
ownership e defaults ainda precisam ser classificados antes de autorizar geração.

Gate de parâmetros em [defaults.rs](../tools/decipher-eval/src/defaults.rs):
`check_reconstruction_defaults` exige sucesso estrutural e percorre todas as listas
de parâmetros, inclusive funções aninhadas, arrows, métodos e funções não selecionadas.
Aceita identificadores simples com defaults de literal primitivo; rejeita defaults
com chamadas/expressões/objetos, destructuring e rest. Fecha conservadoramente o caso
de IIFE/throw observado; não tenta decidir pureza de expressões arbitrárias.
Não executa defaults nem corpos, não gera código e não prova efeitos de chamadas,
propriedades, coerções, aliases ou TDZ interna. Próxima classificação é dos corpos;
o sucesso deste gate também não é autorização para execução.

### Peças offline de invocação e transformação de query (2026-10-22)

Atualização do diagnóstico estrutural: IPC debug counted preserva orçamento entre
processos em ledger append-only com ordinal/lock/fsync, sem reset dos guards antigos.
Teto de classificação/contexto HTML é 1 MiB por script, somente diagnóstico;
descoberta elegível continua 256 KiB, sem ampliar executor. Scripts acima do teto
ou contagem excedida preservam observações dos demais e marcam cobertura parcial.
GET contado 2 analisou os 17 inline sem falhas/limites e observou uma única jsUrl
literal canônica em ytcfg.set no contexto Music Watch. Isso não valida binding,
execução ou o resolver de produção. Aquisição diagnóstica posterior descrita abaixo.

Checkpoint bootstrap→script separado implementado em
[bootstrap_script_probe.rs](../src-tauri/src/sabr/bootstrap_script_probe.rs), somente
debug com bootstrap-inspection. Exige main e orçamento exato de um GET Music + um
script GET; create_new/fsync persistente reserva antes da sessão, sem reset/retry.
Não modifica os ledgers/guards históricos nem o resolver de produção. Seleção em
[script_diagnostic.rs](../tools/decipher-eval/src/script_diagnostic.rs) exige inventário
HTML completo do subconjunto, uma única jsUrl estática/literal/canônica em caminho
Music Watch direto e recusa ancestrais duplicados/spread/computed/accessors. Guarda
referência ao HTML e caminho somente em memória, sem Debug/Serialize. Isso é pista
de aquisição sintática, não prova do ambiente/execução de ytcfg nem elegibilidade
da descoberta antiga. Script solicitado no origin Music, root-relative canônico,
sem cookies/auth/jar, redirects/retries; 20 s/4 MiB para cada GET, HTTP 200/MIME/UTF-8.
Teto 4 MiB para AST diagnóstico do script é separado dos limites de descoberta e
execução, que não foram ampliados. Oxc/HTML não têm teto absoluto de heap/CPU.
Inventário de timestamp conta propriedades signatureTimestamp e literais inteiros
positivos até u32, inclusive em código aninhado/inativo; não compara valores ou
resolve aliases/atribuições/semântica. Aquisição real encontrou duas propriedades,
ambas literais inteiras: não seleciona STS único, não comprova binding ao POST player.
Todo conteúdo descartado ao retornar; sem execução JS, player, SABR ou áudio.

Diagnóstico estrutural opt-in posterior: tools/bootstrap-inspect é fachada somente
de parsers que compartilha os módulos bootstrap/bootstrap_context/bootstrap_html
da crate experimental via path, sem rquickjs/oxc_semantic/HTTP/sessão. Feature
bootstrap-inspection no app é opcional e não default. IPC debug-only novo
debug_youtube_bootstrap_structure_once exige main e orçamento exato {auxiliaryGet:1},
reserva atômica própria antes da sessão, um GET Music limitado e nenhuma operação
player/script/SABR/áudio. Sem feature recusa antes da reserva/rede. Não reseta
comandos históricos. Projeção é html_ast_syntax_only, contagens e bindingValidated=false;
ambiguous indica somente múltiplas propriedades estáticas conhecidas, não elegibilidade.
bootstrap_context reconhece chamadas diretas ytcfg.set({...}) e recursa só objetos
de propriedades estáticas; conta separadamente caminho exato de Music Watch,
caminhos canônicos e membros não suportados. Não prova identidade global de ytcfg,
ordem/execução, ausência de mutações ou vínculo ao POST player. Strings/paths não
saem da memória. IPC não seleciona URL e não altera o resolver de produção.
Tentativa real parou em script_limit; código posterior separa limites de bytes
e quantidade de scripts sem ampliar limites e sem nova execução autenticada.

Experimento de descoberta em [bootstrap.rs](../tools/decipher-eval/src/bootstrap.rs),
somente na crate independente: discover_bootstrap_script recebe um script inline já
identificado e uma chave explicitamente fornecida pelo caller. Parse AST completo,
sem execução; aceita apenas statements diretos ytcfg.set com objeto de propriedades
estáticas e valores primitivos literais. Rejeita statements adicionais, condições,
funções, redefinição local, mutação, spread/getters/chaves computadas e valores com
chamadas, inclusive propriedades não selecionadas. Duas declarações da chave,
mesmo iguais, são ambíguas. Retém referência à fonte, slice exato da propriedade e
caminho decodificado, sem Debug/Serialize. Restringe caminho ao mesmo subconjunto
root-relative da aquisição; limites de fonte 256 KiB, caminho 512 bytes.

PLAYER_JS_URL nas fixtures é **hipótese sintética**, não campo observado ou origem
confirmada. Busca local não encontrou referência adicional que confirme o vínculo;
nenhuma consulta remota nesta rodada offline. Não há parser HTML/seleção de script
inline, coleta entre scripts, validação do ambiente global de ytcfg, associação à
operação HTTP ou integração no app. Mesmo sucesso AST não prova que ytcfg.set real
tenha semântica esperada. O subconjunto rejeita objetos aninhados e scripts mistos;
não ampliar por tentativa cega. Limites de parser continuam não absolutos.
Correção de produção da origem permanece pendente: não substituir assets.js por
uma chave hipotética nem declarar vínculo ao player só por encontrar um caminho.

Aquisição isolada em [player_script.rs](../src-tauri/src/sabr/player_script.rs):
PlayerScriptBinding toma emprestado o payload exato e exige assets.js como caminho
root-relative canônico /s/player/.../base.js, sem query, fragmento ou escapes.
Destino fixo www.youtube.com, sem revisão adivinhada/fallback. Fetch consome o binding,
usa cliente sem cookies/auth/redirect/retry, timeout de 20 s, HTTP 200, MIME JavaScript,
UTF-8 e corpo não vazio de até 256 KiB. Fonte/binding não implementam Debug/Serialize.
AcquiredScript conserva a referência original e permite conferir identidade de ponteiro,
não apenas igualdade de URL/payload. Isso vincula aquisição à declaração, não prova
compatibilidade ou legitimidade semântica do programa. Não há execução JavaScript.
Presença de assets.js no player real não foi observada; ausência falha fechado.
O limite conservador pode rejeitar o script real; não ampliá-lo automaticamente.
Coordenador debug-only inspect_script_once agora exige orçamento exato {scriptGet:1}
com rejeição de campos extras e reserva atômica antes de validar identidade. Uma
tentativa por processo, inclusive falhas; sem reset. Fetch é privado, acessível por
esse coordenador. O orçamento interno só cobre script, não
GET auxiliar/player/SABR/áudio; não é consentimento humano nem controle de origem
de janela. O IPC debug-only debug_youtube_player_script_once em script_probe.rs
impõe main e orçamento exato {auxiliaryGet:1,playerPost:1,scriptGet:1} antes de
qualquer leitura do catálogo/sessão. Reserva tentativa global própria, inclusive
falhas, sem reset. Escolhe faixa do catálogo somente em memória, obtém player novo
e só tenta script se playability for OK. Projeta status/código fixo/bytes/vínculo;
não retorna fonte/URL/credenciais nem executa JS/SABR/áudio. Não há chamada automática.
Fonte adquirida é descartada ao retornar: inspeção não é integração do decipher.
Não reusar autorização histórica.

`invoke_synthetic` na crate experimental avalia fonte sintética cujo resultado é
Function e chama via API tipada rquickjs com três strings explícitas, sem interpolar
argumentos no JavaScript. Compartilha com evaluate_synthetic runtime novo, limites,
deadline e validação estrita da saída; limita cada argumento UTF-8 a 16 KiB. Não
omite defaults, não extrai/reconstrói funções e não aprova fonte remota. Esta API
substitui a pendência de passagem tipada de argumentos, não os gates de efeitos.

[sabr/url_resolution.rs](../src-tauri/src/sabr/url_resolution.rs) implementa helper
puro transform_n, sem HTTP/IPC/sessão. Valida destino HTTPS/subdomínio googlevideo/
videoplayback, exige representação de entrada já normalizada, limita URL a 64 KiB
e desafio/resultado a 16 KiB. Recusa encoding inválido/lossy, n duplicado inclusive
codificado, listas assinadas repetidas e n listado em sparams/lsparams. Envelopes
s/sp/cipher/signatureCipher continuam não suportados. Só altera o valor de n;
preserva nome cru, pares restantes, vazios, duplicatas não relacionadas e ordem.
Erros/saída vazia/enhanced_except_/oversize bloqueiam; revalida destino após alteração.
Sem n não chama o callback. Não altera rn/alr/cver/pot.

O callback ainda não está vinculado a um programa/revisão do mesmo bootstrap;
testes usam somente callbacks sintéticos. Helper não é decipher e não está ligado
ao probe: prepare_url continua bloqueando s/n. Não há nova dependência do executor
no app. Permanecem necessárias reconstrução/efeitos, obtenção e vínculo do script,
integração do transporte e handoff limitado de mídia ao decoder antes de teste audível.

Próxima etapa técnica: ampliar formas suportadas com análise de efeitos/initializers,
implementar reconstrução validada a partir do plano e definir vínculo à identidade do player
antes de integrar qualquer transformação. Inventário lexical não é decipher.
Até lá, prepare_url continua bloqueando s/n e o
comando de tentativa única não muda. Revisão não autoriza download de player nem
novo probe autenticado. Nenhum algoritmo de decipher foi validado nesta etapa.

### Projeto do agendador nativo sem token (2026-10-22)

Decisão: [D-007](decisions.md). Referência de comportamento: `SabrStream` do `googlevideo` (MIT) na revisão
pinada acima; este projeto não copia o código. Estado: **projeto, nada implementado além do probe de
sessão de debug** (`sabr/session_probe.rs`).

**Encaixe.** O motor já recebe corpos UMP por um canal (`AudioEngine::play_ump_receiver`,
`mpsc::Receiver<Result<Vec<u8>, String>>`). O agendador nativo substitui o relay como produtor desse
canal e não toca decoder, saída nem fila (D-001). Módulo novo `sabr/session.rs`, sem `cfg(debug_assertions)`
nem dependência da feature de teste.

**Núcleo.** Uma sessão por ativação, com: bootstrap (`/player`), URL resolvida (`n` transformado,
`cpn` estável, `cver`, `rn` crescente, `alr=yes`), `AbrRequestState` (cookie, contextos,
intervalos de buffer, formatos inicializados, posição) e laço de rodadas: montar request, POST, interpretar
a resposta, empurrar o corpo ao canal, evoluir o estado. A evolução do estado já existe testada
(`advance_state`). Dependências injetadas por trait (fonte do player, transformador de `n`, relógio,
transporte), para testar o laço com respostas gravadas e sanitizadas, sem rede.

**Controles da resposta.** Backoff (teto 5 s), redirect (só googlevideo/HTTPS; reaplicar o `n` se vier
outro desafio), reload (novo `/player`, novo `n`, mesma posição), seek do servidor, fim de faixa,
erro SABR. Recuos exponenciais de 500 ms a 5 s, até 10 tentativas; travamento de 30 s; três
travamentos encerram a sessão como falha.

**Sinais de degradação sem token e reação.**

| Sinal | Reação |
|---|---|
| 401/403 na primeira request | uma recarga do `/player` e nova tentativa; se repetir, relay |
| 401/403 no meio da faixa | uma recarga (URL expirada); se repetir, relay |
| Estado de proteção diferente de OK | contar; ao atingir o limite informado pelo servidor, relay |
| Sem mídia nova por 30 s | travamento; três seguidos, relay |
| Parte de erro SABR ou reload em laço | relay |

A queda para o relay tem granularidade de **faixa** (o relay não faz seek, então recomeça a faixa);
isso é aceito por ora e fica registrado como limitação.

**Pré-requisito: `n` leve.** O solver atual leva cerca de 55 s e 1 GiB, aceitável só em debug. Antes
de ligar o agendador é preciso um `n` rápido e barato. Caminho a medir (não medido): processar o player
uma vez por revisão, em segundo plano, guardar o resultado pré-processado no cache local do player e
resolver cada faixa sobre ele. Se não bastar, o agendador fica atrás de uma configuração desligada.

**Ponto de extensão do token.** `Option<Box<dyn TokenSource>>`, ausente. Quando existir, o token
vai no campo 2 do `StreamerContext`. Nenhuma implementação é planejada aqui; ver D-007.

**Ordem de trabalho.** M0 medir 3 a 5 sessões de debug (aceitação da rodada 0, estados de proteção,
sobrevivência a 6 rodadas e seek). M1 `n` leve verificado contra o oráculo. M2 núcleo do agendador
com testes de respostas gravadas. M3 integração atrás de configuração, desligada por padrão, relay
como padrão. M4 gates de D-002 em execução real (áudio, continuidade, pausa, troca, fim, seek,
expiração). M5 promoção só com evidência dos gates.

## 7. Biblioteca, capas e escritas

- Browse observado em navegação de playlists: POST gzip JSON, top-level request `context`/`browseId`; resposta responseContext/contents/trackingParams/background ou frameworkUpdates. Um replay read-only no mesmo browser retornou 200 com shape equivalente; não prova clone de todos os headers ou replay universal.
- Probe VibeDeck em 2026-10-18 retornou readSucceeded e seis playlists; não valida sozinho cards, busca ou paginação.
- Paginação: contrato `{tracks, hasMore, continuation}`, catálogo persistido por geração; sem cap de 100. Deadlock de mutex reentrante corrigido em 2026-10-01. Runtime mostrou 529 faixas e outra biblioteca com 1.257/12 páginas, incluindo reload persistido. Testes sintéticos cobrem 250 faixas e >1.000 páginas; fixtures atuais de WebView ainda faltam.
- Capas: search/browse pegam última thumbnail, sem comparar dimensões/square; fallback DOM só img.src; viewer reutiliza coverUrl. Qualidade/resolução reais desconhecidas. Não inventar sufixos/upscale; observar só dimensões/host-path sanitizados antes de mudar. Catálogo filtra fontes inválidas/token markers, mas não remove genericamente toda query de imagem.
- Writes: apenas prior art `sigma67/ytmusicapi` sugere playlist/create, browse/edit_playlist e playlist/delete. Remove requer videoId + setVideoId da instância; leads não validados no cliente atual. Sem escrita enviada/contrato confirmado. Futuro serviço precisa tipagem, geração/correlação, timeout/cancel, erros/permissão/duplicado e refresh terminal; sem retries automáticos de operação não idempotente.

## 8. Histórico mínimo para não repetir investigações

| Data | Resultado e lição |
|---|---|
| 2026-10-07 | Browser player OK, seis formatos áudio somente signatureCipher, TTL observado 21.540 s; não foi sucesso do player HTTP nativo |
| 2026-10-07 | Replay ranged POST falhou 400 ou 200 UMP control-only/zero mídia, mesmo correlacionando URI e testando body x+NUL, UA/client hints/headers. Não prova URL universalmente single-use |
| 2026-10-07 | Experimento forced H3 continuou reportando HTTP/1.1; A/B inconclusivo, não prova H3 ineficaz. Feature/guard experimentais removidos; browser fetch H3 funcionou sem provar causa |
| 2026-10-07 | GET legado removendo ump/range voltou a tocar e seek foi confirmado pelo usuário; skip lento/rapid-skip dessincronizado reportados, causas não medidas |
| 2026-10-18 | Hidden WebView recebia 200 UMP sem query ump/mime; filtro rejeitava como missing_ump_flag. Critério mudou para método/origem/path/status/MIME |
| 2026-10-18–19 | One-response: protobuf corrigido, extração chegou a funcionar, mas container probe falhava. Toast genérico não diagnosticava auth; correlação/enum seguro adicionados |
| 2026-10-20 | Trace com init + segmentos completos e ordenados descartou missing-init/truncamento/mistura para aquela resposta; seekability do GrowingReader era falha demonstrada. One-response tocou ~19 s, não faixa completa |
| 2026-10-20 | Relay contínuo + seleção áudio antes de emitir + dedup por header deram áudio multi-response, fim automático/next e pausa validados |
| 2026-10-20 | Gate A nativo UNPLAYABLE; cookies/contexto alterados e sessão recapturada; novo teste ainda não autorizado |

Observações REA de bundle/candidatos estáticos e janelas sem tráfego eram attach-limited/inconclusivas, não prova de endpoints ausentes. Não repetir export de bundles enormes com dados privados; preferir projeção sanitizada na origem.

## 9. Ferramentas e validação histórica

- `npm run tauri:debug`: Vite 1420 + WebView2 CDP loopback 9222 (`VIBEDECK_CDP_PORT`). Exe debug direto depende de Vite; build novo exige fechar processo que trava binário. Não matar/reiniciar por rotina sem necessidade/escopo.
- CDP browser socket exige Target.attachToTarget com flatten e sessionId para Runtime.evaluate; aguardar internals após navegação. Não imprimir targets watch/URLs, cookies ou expressões com IDs.
- `npm run youtube:healthcheck`: app já rodando, não inicia playback; full run faz busca fixa e leitura de biblioteca (exige autorização; pode afetar histórico). Projeções de checks/player/audio/umpCapture, exit 2 em falha. Captura WebView isolada não é necessariamente input do engine.
- `debug_youtube_playlist_probe_status`: Windows debug-only, leitura fixa existente, `{readSucceeded,playlistCount,result}`; sem JS arbitrário/mutação/retry. Launcher opt-in detecta conflito da porta.
- Rodadas de testes e procedimentos de conferência: [validação](validation.md).
- Warnings de dead-code existem em caminhos experimentais (`UnsupportedContainer` entre eles). Código de saída de pipelines PowerShell antigos podia ser ruído; conferir exit do processo real, não apenas filtro de stderr.
- Scripts temporários em `%LOCALAPPDATA%/Temp/opencode` não são documentação durável nem garantidos após restart. Recriar probes sem IDs persistidos quando necessário.
- Pendências gerais: sanitizar logs legados que ainda podem imprimir IDs/URLs/prose, limitar buffer growing, validar reset/cancel/rapid-skip e manter fixtures de parsing/renderers. Não ler/dumpar log inteiro para procurar um único indicador seguro.
