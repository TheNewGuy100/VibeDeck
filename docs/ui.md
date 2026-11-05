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
