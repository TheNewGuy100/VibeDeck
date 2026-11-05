# Arquitetura

Revisado em 2026-10-21 por migração documental. [Índice](README.md).

## Implementação atual

Cliente pessoal leve em Tauri 2, Rust, Vue 3/TypeScript e Vite. A biblioteca local
e o catálogo online compartilham fila e saída de áudio Symphonia → PCM → CPAL.
`lib.rs` e `App.vue` ainda concentram responsabilidades.

| Área | Arquivos principais |
|---|---|
| Comandos, sessão, bridges e bootstrap | `src-tauri/src/lib.rs` |
| Sessão nativa: persistência protegida e autenticação | `src-tauri/src/youtube_session.rs` |
| Captura/restauração Windows e diagnóstico HTTP seguro | `src-tauri/src/youtube_cookie_bridge.rs`, `youtube_http.rs` |
| Diagnóstico de sessão Windows debug-only | `src-tauri/src/youtube_session_debug.rs`, `public/session-debug.html` |
| Decode, UMP, GrowingReader e output | `src-tauri/src/audio/engine.rs` |
| SABR experimental e protobuf manual | `src-tauri/src/sabr/mod.rs`, `proto.rs` |
| Executor JS experimental, dependência opcional somente no trial debug | `tools/decipher-eval/`, feature `native-playback-trial` |
| Coordenador vertical nativo opt-in com ledger persistente | `src-tauri/src/sabr/native_trial.rs` |
| Fachada de parsers para diagnóstico opt-in, sem executor JS | `tools/bootstrap-inspect/`, feature `bootstrap-inspection` |
| Catálogo durável | `src-tauri/src/playlist_catalog.rs` |
| UI e ativação da fila | `src/App.vue` |
| Integração e contratos | `src/services/google-sync.ts`, `src/interfaces/player.ts` |
| Diagnóstico seguro | `src/services/player-diagnostic.ts` |

## Arquitetura planejada

YouTubeClient (metadata/contexto) → FormatSelector → StreamResolver/SABR →
StreamClient (HTTP/buffer) → AudioEngine; Player coordena fila e ativação.
Esses módulos são uma separação desejada, não todos já existentes.
Decoder/output não devem depender de Innertube, cookies ou decipher.
Motivos: [D-001 e D-002](decisions.md).

### Fronteira de autenticação implementada

`YouTubeSession` não depende de Tauri/WebView, não faz HTTP e não inicia playback.
Centraliza leitura/gravação do header protegido via DPAPI, filtro de domínio e
geração de Authorization. Captura interativa, player HTTP, busca, paginação e
restauração dos bridges usam essa fronteira; comandos/janelas continuam em `lib.rs`.
O helper criptográfico DPAPI permanece compartilhado no root.
Credenciais não implementam Debug nem são retornadas ao frontend. A serialização
privada do snapshot só é gravada após proteção DPAPI, com substituição via arquivo
temporário também protegido.

Snapshot v2 preserva nome/valor, domínio normalizado, host-only, path, Secure,
HttpOnly, SameSite e expiração Unix opcional. Cookies de sessão continuam persistidos
como tais (sem prazo inventado); isso não comprova sua validade após reiniciar.
Headers são calculados por URL HTTPS YouTube/path e instante, sem cookies expirados,
com paths mais longos primeiro e nomes repetidos de escopos distintos preservados.
Duplicatas idênticas são removidas; conflito na mesma identidade nome/domínio/path
é erro, não último item vence. Hash usa cookie elegível para o mesmo destino/instante;
valores de auth conflitantes em paths de mesma prioridade são rejeitados, pois o
WebView2 não fornece criação para desempate fiel ao browser.

O adaptador Windows usa WebView2 diretamente: conserva o ponto inicial do domínio
para distinguir host-only de domain-cookie, perdido na conversão Wry/cookie::Cookie.
Restaura flags/expiração e aguarda confirmação antes de navegar. Não limpa nem
sincroniza integralmente a jar viva; cookies renovados pela página podem divergir.
Essa integração nativa ainda não foi exercitada com sessão real.

Headers legados não têm metadados recuperáveis: leitura retorna
`youtube_session_recapture_required`, preservando o arquivo. Disponibilidade local
fica falsa para legado, snapshot inválido ou ausência de auth elegível; não abre
login automaticamente. Uma captura interativa futura grava v2; ver [D-005](decisions.md).
Identidade opcional do usuário pode ser vinculada ao hash; consumidores atuais
passam `None` e não derivam identidade de AuthUser=0. Renovação, cliente HTTP/contexto
e coordenação do login ainda não foram totalmente extraídos. Isso não elimina o relay atual.

O fetch de player usa um cliente HTTP por operação, sem redirects, timeout de 20 s,
corpos limitados a 4 MiB, status HTTP verificado antes de ler corpo e erros de
transporte/JSON/payload em códigos fixos. Logs/probe allowlistam playability; não
exportam erros reqwest, corpos de erro ou valores do config. Não há jar Set-Cookie
compartilhada GET→POST nem renovação de snapshot implementada.

`npm run session:debug` inicia uma página independente desde a primeira navegação,
sem montar `App.vue` ou restaurar biblioteca/fila. O comando
`debug_youtube_session` só é registrado em Windows com debug assertions e aceita
ações fixas da janela `main`, serializadas sem fila/retry. Login entra diretamente
na página externa, sem montar a aplicação na janela de autenticação. Captura/salva
é explícita e substitui o snapshot somente após auth local elegível; verifica a
releitura protegida. Restauração usa WebView privada em `about:blank`, recusa baseline
com cookies YouTube e bloqueia navegação para outros destinos. Compara valores e
todos os metadados em memória, expondo somente contagens/booleans/códigos fixos.
A janela de teste recebe fechamento ao sair, inclusive em erro; não limpa a jar
normal nem navega ao provedor. A ferramenta não comprova aceitação pelo servidor.
Os adaptadores de produção são reutilizados sem alterar playback ou login normal.

## Contratos de fila e catálogo

- Catálogo é fonte durável de hidratação/fila. Mesclar por geração e ID, persistir antes de avançar cursor; inicializar geração frontend pelo catálogo persistido.
- Paginação tem um dono/in-flight por playlist/geração; pedidos durante carga inicial aguardam o mesmo trabalho. Sem cap artificial de 100 faixas ou número fixo de páginas.
- Finalizar em página vazia/repetida/duplicada, cursor inválido/repetido ou terminal do provedor. Falha de persistência deve rejeitar. Overlay só some no terminal.
- Usar request ID, identidade da playlist, timeout/cancelamento e rejeição de respostas antigas. Não manter mutex ao chamar helper que adquire o mesmo mutex.
- Ativações levam sequência, geração e índice. Resultado/snapshot antigo não substitui intenção nova. Abort frontend não cancela automaticamente invoke nativo.

## Identidade Windows

`src-tauri/tauri.conf.json` define produto/descrições/`com.vibedeck.desktop`;
Cargo/binário `vibedeck`. Janela `main` com título VibeDeck; auxiliares
`youtube-auth`, `youtube-stream-bridge` com títulos prefixados.
Sem sidecar. WebView2 conserva o processo `msedgewebview2.exe`, controlado pelo runtime.
