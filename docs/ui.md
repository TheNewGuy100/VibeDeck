# UI, acessibilidade e visualizadores

Revisado em 2026-10-21 por migração documental. [Índice](README.md).
Contratos abaixo são requisitos; cobertura parcial não significa validação integral.

## Biblioteca e importação

- Importação local: alvo por teclado com estados vazio/loading/erro/importação; progresso/cancelamento/resumo em `role="status"`/`aria-live`. Playlist online não oferece drop local.
- `Escolher pasta`: diálogo nativo em Tauri, `webkitdirectory` só fora dele; cancelamento silencioso, caminho ausente pelo toast existente.

## Regras visuais e acessibilidade

- Marca lateral: `public/vibedeck-logo.png` transparente, `object-fit: contain`. Ícone Windows mantém badge circular de alto contraste.
- Faixa atual preserva índice estável, badge, favorito, duração e grid; marcador dentro da borda. Estados normal/current paused/current playing/hover/focus/reduced-motion; `aria-current="true"` e anúncio de play/pause.
- Não aninhar elementos interativos: ação da faixa, favorito e capa têm comportamento de teclado separado. Clique/teclado da capa real não inicia playback; fallback decorativo não abre viewer.
- Modais/settings: semântica de diálogo, Escape e focus-visible; foco gerenciado e restaurado. Acessibilidade completa de settings ainda é lacuna.
- Overlays ficam na região de conteúdo dona e visíveis até resposta terminal. Tooltips não capturam ponteiro, escapam stacking contexts e são limitados ao viewport.
- Background visualizer é fixo e não participa do layout da playlist.
- `TrackArtworkViewer`: props `track`/`origin`, evento `close`, Teleport para body, trava scroll, trap de Tab, restaura foco na capa; fecha por Escape/backdrop/botão. Anima do retângulo de origem para card glass de até 450 px, arte quadrada viewport-safe (imagem atualmente até 380 px), backdrop blur/dim, tema claro próprio.
- Tilt/gloss: coordenadas normalizadas/clamp, suavização por um RAF; reduced-motion desliga tilt/transform de abertura/lift. Testes em `src/components/track-artwork-viewer.test.ts`.
- Controles numéricos sempre limitados. Serpents: grupos Cobras/Iluminação só em `kind="serpents"`, fieldsets/select/range/button nativos; painel geral possui scroll, canvas não tem overlay interativo. Preferências Rust revisionadas são fonte compartilhada.

## Componentes e cobertura

| Componente | Responsabilidade / cobertura |
|---|---|
| `VisualizerCanvas` | Canvas, histórico e inspeção; runtime Canvas ainda necessário |
| `VisualizerControls` | Grupos/limites/overflow; `visualizer-controls.test.ts` |
| `FrequencyRangeControl` | Min/max e handles cruzados; teste dedicado pendente |
| `VisualizerPicker` | Seleção/preview/responsividade; interação pendente |
| `PlaylistLoadingOverlay` | Bloqueio até terminal; `playlist-loader.test.ts` |
| `PlaylistShuffleButton` | Shuffle vazio/único/habilitado; interação pendente |
| `TrackArtworkViewer` | Modal/tilt/octantes/foco/reduced-motion; testes dedicados |
| `PlayerSandboxFrame` | Teste manual isolado; testes dedicados e de diagnóstico/bridge |

## Sandbox de diagnóstico

- Debug de sessão separado: `public/session-debug.html`, iniciado por `npm run session:debug`, sem montar a UI principal. Botões nativos, foco visível, status anunciado, confirmação antes de substituir snapshot e controles desabilitados durante cada operação. Não é o sandbox de playback abaixo; roteiro e limites em [validação](validation.md). Validação visual/teclado em Tauri ainda pendente.

- Apenas em settings Developer com modo developer ativo. `srcdoc`, `sandbox="allow-scripts"`, sem allow-same-origin/autoplay; CSP nega rede/mídia. Sem Tauri, cookies ou sessão dentro do iframe.
- Mensagem de request contém só versão e ID válido de 11 caracteres. Host valida schema, `event.source`/WindowProxy exato, conexão e busy; usa fluxo normal de ativação. Substitui fila e pode emitir som: nunca disparar automaticamente, não persistir ID.
- Host só retransmite diagnóstico allowlistado da mesma ativação; snapshot antigo não é atual. `safeSummary` redige texto inseguro; fallback de erro usa código/etapa fixos, nunca erro cru.
- Cobertura: `player-sandbox-frame.test.ts`, `player-diagnostic.test.ts`, `google-sync.test.ts`.

## Contrato dos visualizadores

- Analyzer: callback CPAL, rustfft de 2048 amostras/hop 1024, 32 bandas normalizadas (~21,3 ms a 48 kHz / 23,2 ms a 44,1 kHz).
- Aquisição separada do render: polling Tauri 33 ms, no máximo um `get_audio_levels` pendente; render em RAF, alvo 60 FPS/16,67 ms.
- Spectrograma: captura configurável (default 20 ms), histórico limitado a 10 s; não copiar histórico por frame nem reescrever timestamps ao mudar cadência.
- `active` reage ao áudio; paused/idle têm ambient sutil sem inventar níveis/histórico. Limpar níveis/colunas anteriores ao pausar/sem faixa. Reduced-motion desliga ambient e renderiza uma vez.
- Instâncias invisíveis renderizam uma vez e param RAF. Cache de contexto/gradientes quantizados, buffers de pontos reutilizados. DPR Canvas até 1,5; Serpents3D default/cap 1,25; `getStats()` expõe último/médio/máximo e orçamento.
- Barras: verde até 50% da altura, transição suave ao vermelho entre 51–100%, vermelho só no topo, nunca overlay sólido.
- Tooltip hoje estima bandas log 20 Hz–24 kHz; preferir limites/sample rate reais do analyzer. Canvas inspecionável requer wrapper acessível/equivalente de teclado.
- Riscos não resolvidos: FFT/alocações no callback, latência do polling, smoothing inconsistente, globals por faixa, draw por célula; FPS sustentado em GPU real não medido. A antiga afirmação “decoder sempre espera download completo” não descreve o GrowingReader incremental atual.
- Procedimento de teste: [validação](validation.md).
- Logs (modo desenvolvedor): drawer em tela cheia (100vw x 100dvh) que sobe de baixo, aberto pelo botao `Logs` do cabecalho; Esc fecha e devolve o foco. Fluxo cronologico (mais antigo em cima) que segue o fim enquanto o leitor esta no fim; rolar para cima pausa e mostra `N novos`. Filtros: busca, nivel e fontes (chips com contagem). Linha expande para detalhes/copiar. Sem botao Limpar (o buffer de 2000 eventos gira sozinho); Copiar e Exportar no lugar. Aba `Janelas` lista WebViews.
- Niveis de log: debug, info, warn, error. O filtro e por severidade minima (Tudo, Info+, Avisos+, Erros). O backend infere o nivel pela mensagem (warn: retry/fallback/stale/skipped/blocked/incomplete/renew; debug: snapshot/hydrated) e um nivel explicito em `writeAppLog`/`logUi(msg, level)` vence a inferencia.
- Favoritos: persistidos no nativo (`favorites.json`), uma entrada por fonte + id. A UI deriva tres listas da mesma fonte: `favorites` (Global), `favorites-local` e `favorites-youtube`; nao aceitam importacao nem configuracao. Favoritar atualiza a tela na hora e confirma com o nativo (o ultimo toque vence; falha reverte e avisa). Faixa local favoritada cuja copia foi apagada continua listada como indisponivel.
- Fontes (tela): hub de bibliotecas locais (cards com faixas e tamanho; criar, renomear, excluir, importar aqui), banner de arquivos sem uso com revisao antes de apagar, cards dos tres favoritos e estado da conta do YouTube. Dialogos com semantica de dialogo, foco no campo, Esc fecha e devolve o foco, validacao de nome igual a do nativo. A barra lateral agrupa FAVORITOS, BIBLIOTECAS LOCAIS e YOUTUBE MUSIC; `Todas as locais` e a visao agregada. Pasta escolhida no seletor vira biblioteca com o nome dela (subpastas opcionais); soltar arquivos sobre uma biblioteca importa para ela.
- Indisponibilidade: `unavailableReason` (`signed-out`, `file-missing`) em `LibraryItem`, com `isTrackUnavailable`/`playableTracks` em `services/track-availability.ts`. Faixa indisponivel continua listada (esmaecida, selo e dica), fica fora de toda fila (proxima, anterior, repeticao, aleatorio) e nao inicia por clique. Favoritos do YouTube ficam listados com `signed-out` enquanto a conta nao esta conectada e voltam ao reconectar.
- Sair da conta: o nativo apaga sessao e catalogo de playlists (inclui `.bak`, `.tmp`, `.corrupt-*`); o app limpa busca, cache de carga e, se tocava do YouTube, para o audio. Ao abrir, o catalogo em cache so e mostrado com sessao valida.
- Login do YouTube Music: a espera (`services/youtube-login.ts`) aguarda 1,5 s antes da primeira checagem e tolera ate 8 erros seguidos de "janela ainda nao pronta" (`youtube_cookie_capture_*`); janela fechada ou outro erro encerra na hora com mensagem propria, e o app nao abre uma segunda janela depois de um login cancelado. Logs em `youtube-auth`: inicio, janela aberta, retentativas (aviso), progresso a cada 30 s, conectado ou falha com codigo e tempo. Codigos nativos nunca levam URL, cookie ou conta.
- Login: cookies de dominio (`.youtube.com`) e de host (`youtube.com`) com o mesmo nome e caminho sao cookies distintos e ambos sao mantidos; so identidade igual (incluindo host_only) com valor diferente continua falhando (`youtube_session_ambiguous_cookie`), com um registro dos atributos em conflito (nome, dominio, caminho, flags; nunca valores). A espera nao encerra por erros de conteudo de cookie (ambiguo/invalido): segue ate o limite de tempo e entao reporta o codigo.
- Login: copias do mesmo cookie (mesma identidade e mesmo valor) que so diferem em validade ou flags, sobras de sessoes anteriores, sao mescladas: vale a Secure e, entre elas, a de maior validade. So valores diferentes sob a mesma identidade falham como ambiguos (o log mostra atributos em conflito, nunca valores).
- Login: cookies de autenticacao (SAPISID, SID, __Secure-*PSID*, LOGIN_INFO etc.) com copias de valores diferentes continuam reprovando a captura. Qualquer outro cookie (ex.: `__Secure-YNID`, `VISITOR_INFO1_LIVE`) com copias divergentes e descartado inteiro, nao enviado, e o log lista so os nomes descartados.
- Fontes (organizacao): a tela abre com um card por fonte (Biblioteca local, YouTube Music, Favoritos) em formato de abas (setas, Home e End navegam; a escolha fica salva) e lista abaixo o conteudo da fonte escolhida. O bloco FONTES saiu da barra lateral; YouTube Music mostra estado da conta, conectar/desconectar e as playlists sincronizadas.
