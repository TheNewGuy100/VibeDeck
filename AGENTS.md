# Contexto do projeto — Cliente YouTube Music otimizado em Tauri/Rust

## Objetivo

Criar um cliente pessoal e leve para reprodução de músicas do YouTube Music usando:

- Tauri
- Rust no backend
- React/TypeScript no frontend
- Player de áudio próprio
- Sem depender do IFrame Player como mecanismo principal de reprodução
- Foco em baixo consumo de RAM/CPU e controle completo do pipeline de áudio

A investigação atual está analisando o comportamento observado do cliente web do YouTube Music (`WEB_REMIX`), especialmente o endpoint interno:

POST /youtubei/v1/player

IMPORTANTE:
`youtubei`/Innertube e os mecanismos descritos abaixo são interfaces internas do YouTube, não uma API pública estável. Podem mudar e seu uso deve considerar os termos/restrições aplicáveis do serviço.

---

# Fluxo identificado

O fluxo técnico observado é:

videoId
  ↓
/youtubei/v1/player
  ↓
streamingData
  ↓
adaptiveFormats
  ↓
selecionar representação audio/*
  ↓
url ou signatureCipher
  ↓
resolver URL temporária
  ↓
googlevideo.com/videoplayback
  ↓
stream HTTP incremental
  ↓
demux MP4/WebM
  ↓
decode AAC/Opus
  ↓
PCM
  ↓
saída de áudio

O objetivo final da API interna do aplicativo seria permitir algo conceitualmente semelhante a:

play(video_id).await?;

Todo o processo de obtenção/resolução do stream ficaria escondido atrás dessa abstração.

---

# Evidência observada em /youtubei/v1/player

Uma resposta real do endpoint apresentou:

playabilityStatus.status = "OK"

e:

audioOnlyPlayability.audioOnlyAvailability =
"FEATURE_AVAILABILITY_ALLOWED"

Além disso:

streamingData.expiresInSeconds = 21540

O campo:

streamingData.adaptiveFormats

contém representações independentes de áudio.

Foram observadas, entre outras:

itag 140
mimeType: audio/mp4; codecs="mp4a.40.2"
AAC
~129 kbps
44100 Hz
2 canais
AUDIO_QUALITY_MEDIUM

itag 141
mimeType: audio/mp4; codecs="mp4a.40.2"
AAC
~258 kbps
44100 Hz
2 canais
AUDIO_QUALITY_HIGH

itag 249
mimeType: audio/webm; codecs="opus"
Opus
~49 kbps
48000 Hz
2 canais
AUDIO_QUALITY_LOW

itag 250
mimeType: audio/webm; codecs="opus"
Opus
~63 kbps
48000 Hz
2 canais

Isso demonstra que não é necessário obter um stream contendo vídeo e depois extrair o áudio. Existem representações audio-only diretamente em adaptiveFormats.

---

# signatureCipher

Nos formatos observados, a localização da mídia veio através de:

signatureCipher

em vez de uma `url` imediatamente utilizável.

Estrutura conceitual:

signatureCipher
 ├── url
 ├── s
 └── sp

Exemplo:

sp=sig

Portanto o processo observado é conceitualmente:

s
 ↓
signature decipher
 ↓
assinatura resolvida
 ↓
adicionar como parâmetro indicado por sp

No caso observado:

sp = "sig"

logo:

sig = decipher(s)

A lógica responsável por essa transformação é associada ao player JavaScript atual do YouTube e pode mudar.

Por isso ela NÃO deve ficar acoplada ao restante do aplicativo.

---

# Parâmetro n

Também existe um parâmetro:

n=...

na URL de videoplayback.

Ele possui lógica de transformação separada da transformação de `s`.

Portanto um StreamResolver precisa conceitualmente considerar:

signatureCipher
       │
       ├── s → signature decipher → sig
       │
       └── URL
              │
              └── n → n transform

Não assumir que resolver somente `s` é suficiente.

---

# Requisição real observada para googlevideo

Foi capturada uma requisição do YouTube Music para:

https://<cdn>.googlevideo.com/videoplayback

Ela utilizava:

itag=140
mime=audio/mp4
clen=6657472
dur=411.238

Logo tratava-se de um stream AAC/MP4 audio-only.

Também estavam presentes parâmetros como:

c=WEB_REMIX
cver=1.20260920.16.00

sig=...
n=...
pot=...
cpn=...

range=0-66739
rn=152
rbuf=0

ump=1
srfvp=1

e vários parâmetros adicionais relacionados à entrega da CDN.

A requisição observada era:

POST /videoplayback

com body:

x

Headers incluíam:

Referer: https://music.youtube.com/
Origin: https://music.youtube.com/
User-Agent: Firefox

---

# Range / streaming incremental

Um detalhe importante da requisição observada:

range=0-66739

Apesar de o conteúdo completo possuir:

clen=6657472

o cliente solicitou inicialmente aproximadamente 65 KB.

Portanto o player trabalha com obtenção incremental/buffering em vez de necessariamente baixar a música inteira antes da reprodução.

Pipeline desejado:

googlevideo CDN
      ↓
HTTP range/chunks
      ↓
buffer
      ↓
demux
      ↓
decoder
      ↓
PCM
      ↓
output

Enquanto o áudio é reproduzido, novos dados podem ser obtidos conforme necessário.

---

# Parâmetro pot

A requisição real também apresentou:

pot=<token>

Não assumir que `s` + `n` constituem todo o mecanismo necessário.

`pot`, `cpn` e outros parâmetros/contextos de reprodução podem fazer parte das exigências atuais do serviço.

O resolver deve ser projetado de maneira modular justamente porque esses mecanismos internos podem mudar.

---

# Arquitetura proposta

Separar completamente integração com YouTube da reprodução de áudio.

## YouTubeClient

Responsável por:

- pesquisa
- obtenção de metadata
- chamada /youtubei/v1/player
- parsing de streamingData
- parsing de adaptiveFormats

Interface conceitual:

YouTubeClient
 ├── search()
 ├── player(video_id)
 └── get_audio_formats()

---

## FormatSelector

Responsável por escolher a representação apropriada.

Deve considerar:

- mimeType
- codec
- bitrate
- audioQuality
- sampleRate
- channels
- contentLength

Exemplo de preferência futura:

AUDIO_QUALITY_HIGH
    ↓
Opus ou AAC conforme suporte/qualidade
    ↓
maior bitrate adequado

Não assumir que um itag específico sempre estará disponível.

---

## StreamResolver

Responsável SOMENTE por transformar AudioFormat em um stream reproduzível.

Conceitualmente:

trait StreamResolver {
    fn resolve(
        &self,
        format: &AudioFormat
    ) -> Result<ResolvedAudioStream>;
}

Responsabilidades potenciais:

- interpretar `url`
- interpretar `signatureCipher`
- signature decipher
- transformação de `n`
- tokens/contexto necessários
- detectar expiração
- retornar URL final

Toda lógica instável/específica do YouTube deve ficar aqui.

---

## ResolvedAudioStream

Modelo sugerido:

pub struct ResolvedAudioStream {
    pub url: String,
    pub mime_type: String,
    pub codec: AudioCodec,
    pub bitrate: u32,
    pub sample_rate: u32,
    pub channels: u8,
    pub content_length: u64,
    pub expires_at: Option<u64>,
}

O restante do player NÃO deve saber como a URL foi obtida.

---

## StreamClient

Responsável por HTTP.

Sugestão:

reqwest

Responsabilidades:

- leitura incremental
- buffering
- ranges
- retry
- reconexão
- controle de expiração
- prefetch

Fluxo:

ResolvedAudioStream
       ↓
StreamClient
       ↓
bytes
       ↓
AudioEngine

---

## AudioEngine

Não deve possuir nenhuma dependência conceitual do YouTube.

Entrada:

stream de bytes

Pipeline:

bytes
 ↓
demux
 ↓
AAC/Opus
 ↓
decoder
 ↓
PCM
 ↓
CPAL
 ↓
Windows Audio

Deve futuramente suportar:

- play
- pause
- seek
- volume
- buffering
- posição atual
- duração
- troca de faixa
- preloading da próxima faixa

A mesma engine poderia futuramente tocar arquivos locais ou outras fontes.

---

# Estratégia de implementação

NÃO começar implementando signatureCipher.

Primeiro provar o pipeline de reprodução utilizando uma URL googlevideo já resolvida/capturada.

## Fase 1 — Audio pipeline

URL googlevideo já resolvida
 ↓
reqwest
 ↓
leitura incremental
 ↓
MP4 demux
 ↓
AAC decode
 ↓
PCM
 ↓
CPAL
 ↓
som

Objetivo:

reproduzir a URL temporária capturada sem baixar a música inteira previamente.

---

## Fase 2 — Player response

Implementar:

videoId
 ↓
/youtubei/v1/player
 ↓
streamingData
 ↓
adaptiveFormats
 ↓
FormatSelector

Ainda pode usar uma estratégia temporária para resolução do stream.

---

## Fase 3 — StreamResolver

Implementar de forma isolada:

signatureCipher
 ↓
resolver assinatura
 ↓
tratar `n`
 ↓
tratar contexto/tokens necessários
 ↓
ResolvedAudioStream

Essa é a parte mais instável e deve permanecer desacoplada.

---

## Fase 4 — Pipeline completo

Objetivo final:

play(videoId)

internamente:

videoId
 ↓
YouTubeClient
 ↓
PlayerResponse
 ↓
FormatSelector
 ↓
StreamResolver
 ↓
StreamClient
 ↓
AudioEngine
 ↓
speakers

---

# Organização Rust sugerida

src-tauri/src/

youtube/
    mod.rs
    client.rs
    models.rs
    format_selector.rs

stream/
    mod.rs
    resolver.rs
    client.rs
    buffer.rs

audio/
    mod.rs
    engine.rs
    decoder.rs
    output.rs

player/
    mod.rs
    player.rs
    queue.rs
    state.rs

commands/
    player_commands.rs

---

# Princípio arquitetural importante

YouTubeClient conhece YouTube.

StreamResolver conhece detalhes de resolução de streams do YouTube.

StreamClient conhece HTTP.

AudioEngine conhece áudio.

Player coordena tudo.

Evitar:

AudioEngine → YouTube
AudioEngine → Innertube
AudioEngine → signatureCipher

Preferir:

YouTube → ResolvedAudioStream → AudioEngine

Isso reduz drasticamente o impacto quando o YouTube alterar seu player ou protocolo interno.

---

# Estado atual da investigação

Já temos:

[OK] Resposta real de /youtubei/v1/player
[OK] adaptiveFormats
[OK] streams audio-only
[OK] AAC
[OK] Opus
[OK] signatureCipher
[OK] URL real googlevideo observada
[OK] `sig` após resolução
[OK] presença de `n`
[OK] presença de `pot`
[OK] comportamento com range
[OK] tamanho/duração/codec observados

Ainda precisamos investigar/implementar:

[ ] protótipo Rust da Fase 1
[ ] streaming HTTP incremental
[ ] demux MP4
[ ] decode AAC
[ ] CPAL
[ ] gerenciamento de buffer
[ ] seek
[ ] seleção automática de formato
[ ] parsing completo de PlayerResponse
[ ] StreamResolver
[ ] comportamento atual de signature decipher
[ ] transformação atual de `n`
[ ] papel/requisitos de `pot`
[ ] renovação do stream quando a URL expirar

---

# Próximo passo recomendado

Implementar SOMENTE a Fase 1.

Não implementar Innertube nem signature decipher ainda.

Criar um pequeno programa Rust que receba uma URL googlevideo já resolvida e execute:

HTTP incremental
 → buffer
 → demux MP4
 → AAC decode
 → PCM
 → CPAL

Depois que isso estiver funcionando, integrar o endpoint `/youtubei/v1/player`.

Dessa forma, problemas de áudio/streaming ficam separados dos problemas de resolução específicos do YouTube.

---

# Skills obrigatórias do projeto

As skills globais abaixo devem ser usadas quando o trabalho corresponder ao seu escopo:

## visual-components

Usar ao criar ou alterar componentes visuais, regras de design, estados de UI, layouts responsivos, loading, acessibilidade ou testes de componentes.

Responsabilidades:

- Ler as regras visuais existentes antes de editar.
- Definir props, eventos, estados e comportamento responsivo.
- Registrar novas regras e componentes reutilizáveis na documentação visual do projeto.
- Criar testes de interação, estados de loading/erro/vazio e acessibilidade.
- Validar desktop, mobile, tema e overflow quando aplicável.

## interface-flows

Usar em fluxos entre Vue, Tauri, Rust, WebViews, workers, threads, áudio nativo, reloads e APIs assíncronas.

Responsabilidades:

- Desenhar o fluxo antes da implementação.
- Definir fonte de verdade, contratos tipados, eventos, erros e terminal conditions.
- Tratar startup, loading, paginação, retry, cancelamento, reload, reconnect e respostas fora de ordem.
- Criar testes unitários de transição, testes de bridge/API e testes de integração.
- Criar ou documentar smoke tests reais para Tauri, WebView, OAuth e áudio nativo.
- Registrar logs com correlação sem expor tokens ou dados sensíveis.

## visualizers

Usar ao alterar FFT, Canvas, WebGL, espectrogramas, visualizadores de áudio, tooltips, controles ou performance de renderização.

Responsabilidades:

- Documentar fonte do analyzer, sample rate, bandas, cadence, smoothing e pausa.
- Manter aquisição de áudio separada da renderização.
- Limitar histórico, alocações e custo por frame.
- Testar pausa, histórico, intensidade, hover, troca de visualizer e layout de background.
- Validar stacking context, overlays, tooltips e comportamento responsivo.

## youtube-api-analysis

Usar ao investigar ou alterar APIs internas do YouTube Music, Innertube, WebView bridge, paginação, PlayerResponse, streams ou resolução de URLs.

Responsabilidades:

- Registrar endpoint, client context, payload, resposta, paginação, erros e data da observação.
- Separar fatos observados, hipóteses, limitações e comportamento esperado.
- Atualizar o registro de APIs e relatórios técnicos do projeto.
- Criar fixtures sanitizadas e testes de contrato para parsers e bridges.
- Nunca registrar cookies, tokens, assinaturas, identificadores pessoais ou URLs sensíveis.
- Tratar interfaces internas do YouTube como instáveis e não públicas.

## Regra de validação

Toda alteração relevante deve informar quais testes foram executados, quais fluxos foram validados em runtime e quais limitações permaneceram. Build passando sozinho não equivale a validação funcional ou visual.
