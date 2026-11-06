# Decisões

Consolidadas em 2026-10-21 a partir das regras existentes. [Índice](README.md).
As decisões D-001 a D-004 foram consolidadas de regras existentes.

## D-001 — Áudio próprio e cliente leve

- **Estado:** aceita.
- **Contexto:** baixo consumo de RAM/CPU e controle do pipeline são requisitos.
- **Decisão:** manter saída nativa Symphonia → PCM → CPAL, separada do protocolo do provedor.
- **Consequência:** não acoplar decoder/output a cookies, Innertube ou decipher.
- **Revisão:** somente com nova decisão explícita sobre requisitos e medidas de consumo.

## D-002 — SABR nativo como destino; relay como transição

- **Estado:** aceita; substituto ainda experimental.
- **Contexto:** relay funciona, mas a página ainda controla entrega e scheduling.
- **Decisão:** eliminar brokers/WebViews do playback e implementar scheduling/seek em Rust.
- **Consequência:** preservar fallback até validar áudio, continuidade, pausa, troca, fim, seek e expiração. Eventual atestação fora do playback é hipótese, não arquitetura implementada.
- **Revisão:** mediante evidência dos gates e decisão explícita; sucesso de parser/build não basta.

## D-003 — Protobuf/base64 manuais no experimento SABR

- **Estado:** adotada no experimento.
- **Contexto:** bootstrap e protocolo ainda em investigação.
- **Decisão:** helpers manuais, sem `prost-build`/`protoc`.
- **Consequência:** manter testes sintéticos e limites de parsing; não confundir schema reverso com contrato oficial.
- **Revisão:** quando manutenção/compatibilidade exigir avaliar outra abordagem.

## D-004 — Documentação por responsabilidade

- **Estado:** aceita em 2026-10-21.
- **Contexto:** regras, cronologia e evidências repetidas geravam contradições.
- **Decisão:** um dono por informação, navegação pelo [índice](README.md), redirecionamento do guia antigo.
- **Consequência:** atualizar páginas existentes, sem acumular relatórios por sessão.
- **Revisão:** dividir uma página somente quando houver uma responsabilidade independente clara.

## D-005 — Snapshot com escopo; legado exige recaptura

- **Estado:** implementada em 2026-10-21, validação apenas offline.
- **Contexto:** o header legado perdeu domínio/path/validade; sua migração automática ampliaria escopos ou inventaria validade.
- **Decisão:** persistir snapshot v2 protegido, respeitar escopo/expiração e rejeitar uso do legado sem reescrevê-lo. Recuperação exige captura interativa, nunca automática.
- **Consequência:** uma sessão antiga deixa de ser considerada disponível até nova captura; o fallback de playback continua no código, mas também exige snapshot aceito. Detalhes na [arquitetura](architecture.md).
- **Revisão:** somente com fonte confiável de metadados e validação nativa da captura/restauração, sem deduzir escopo de nome/valor.

## D-006 — Observação sanitizada e cache local do player

- **Estado:** aceita em 2026-10-22; implementada e testada offline, sem execução contra o provedor.
- **Contexto:** a engenharia reversa dependia de leitura estática de um script ofuscado e de tentativas contadas que perdiam informação (categoria de status, forma da request). Faltava um oráculo da request real do navegador e uma fonte local para iterar sem voltar ao navegador a cada hipótese.
- **Decisão:** (1) o formato sanitizado de uma request (nomes de parâmetros, números de campo do protobuf, tipos de fio, classes de tamanho) é gerado igual no Rust e no JS, para comparar nativo e navegador; (2) toda tentativa contada devolve a projeção máxima mesmo na falha, e hipóteses viram parâmetros do IPC de debug (`allowRawN`, `dryRun`) em vez de edições de código; (3) rastreio dinâmico usa logpoints por CDP, com expressões restritas e saída revalidada contra um vocabulário fixo; (4) o script **público** do player pode ficar em cache local fora do projeto, com validade de 24 h e até 3 revisões.
- **Consequência:** nenhuma ferramenta emite valores, tokens, URLs completas nem trechos de fonte. O cache não é cofre e não criptografa: jamais recebe cookies, tokens ou respostas de API, e é recusado se cair dentro do projeto. A regra de descartar fontes continua valendo para qualquer dado privado e para o repositório.
- **Revisão:** se o cache ou as expressões de logpoint criarem risco real, ou se o provedor passar a tratar o script como não público.

## D-007 — SABR nativo sem PO token por ora; relay como rede de segurança

- **Estado:** aceita em 2026-10-22; só projeto, nada implementado.
- **Contexto:** o player real manda um token de 85 bytes por faixa no `StreamerContext`. O app não o produz, e produzi-lo fora de um navegador real contornaria a verificação anti-bot do provedor, o que o projeto não faz. Nas tentativas sem token, 1 de 3 recebeu mídia e 2 receberam 401/403 na primeira request, com causa não isolada. Pela referência pública (`SabrStream`), a falta do token aparece como estado de proteção da resposta, com limite de rejeições, e não necessariamente como corte imediato.
- **Decisão:** projetar o agendador nativo para operar sem token, tratar como sinais de degradação o 401/403, o estado de proteção diferente de OK, o travamento e a expiração, e cair para o relay quando eles persistirem. Deixar um ponto de extensão (`TokenSource` opcional, hoje ausente) sem implementação. Nenhum minter, emulador de navegador ou script de terceiros que simule um navegador entra no app.
- **Consequência:** o relay (WebView real, áudio nativo) continua sendo o caminho padrão até os gates de D-002. Se a medição mostrar que o token é necessário, a fonte dele só pode ser o player genuíno, e isso exige nova decisão e revisão de desenho.
- **Revisão:** quando a medição de sessões (taxa de aceitação da rodada 0, estados de proteção, sobrevivência a 6 rodadas e seek) existir, ou se o provedor mudar o comportamento.

## D-008 — Android e TV são metas; relay não é plano B delas

- **Estado:** aceita (2026-11-05, pedido do usuário).
- **Contexto:** o relay depende de WebView2 (captura de resposta, Windows) e de WebView escondido tocando. Não há equivalente para ler corpo de POST no Android/iOS, e várias TVs não têm WebView utilizável.
- **Decisão:** o destino é o cliente SABR nativo em Rust para Windows, Android e TV. Relay continua só como fallback no Windows.
- **Consequência:** fonte de token fica restrita a: (a) cliente sem exigência, se a conta permitir (teste do `tv` autenticado, Premium); (b) WebView genuíno só para gerar o token no Android (sem capturar tráfego); (c) TV sem WebView depende de (a). Nenhum emulador de navegador (ver D-007).
- **Revisão:** após o teste do `tv` autenticado e a checagem de Premium.

## D-009 — Suportar os dois modos de entrega: faixa de bytes (UMP) e SABR completo

- **Estado:** aceita (2026-11-05, acordo com o usuário); só projeto.
- **Contexto:** o player web de 22/10 usava SABR completo (corpo com `StreamerContext`, token de 85 bytes); o de 05/11 usa só UMP por faixa de bytes (corpo de 2 bytes, `pot`, `sig`, `n` na URL) em áudio e vídeo, 60 de 60 POSTs. O servidor anuncia os dois. A causa da troca é desconhecida (revisão, experimento ou conta).
- **Decisão:** o motor nativo terá uma camada comum (URL válida, separação de partes UMP, decodificação) e dois agendadores: por faixa de bytes (caminho principal, testável ao vivo hoje) e SABR completo (alternativa, testada por fixtures offline e sentinela de modo). A escolha do modo vem do que o player real faz e do que a resposta `/player` oferece, com queda para o outro modo. O relay (WebView2) não é fallback geral: não existe em Android e TV (D-008); fica só como ponte opcional no Windows até os gates da D-002.
- **Consequência:** manter `session-observe.cjs` como sentinela para notar quando o player voltar ao SABR e gravar uma sessão nova; a fonte do token (`pot`/campo 2) segue a restrição da D-007/D-008.
- **Revisão:** após o teste de um pedido por faixa de bytes sem `pot` e a próxima observação de SABR.

### D-009, andamento (2026-11-05)

O modo por faixa de bytes está implementado e integrado como caminho principal atrás da feature `native-range-playback` (módulos `native_range` e `native_range_session`, comandos `play_youtube_audio_native_range` e `is_native_range_available`, seek via `seek_audio`). A interface o tenta primeiro e cai
no relay se ele não iniciar. O SABR completo continua sem implementação de produção (só sondas de depuração). Android e TV ainda não foram exercitados: o caminho nativo não usa WebView, mas o motor de áudio e a saída (cpal/Symphonia) precisam de revisão por plataforma.

## D-011 — Bibliotecas locais gerenciadas e favoritos globais

- **Estado:** aceita (2026-11-06, acordo com o usuário); só projeto, sem implementação.
- **Contexto:** o catálogo local (`local_library`, versão 1) só importa para uma playlist por nome e remove faixa; não há criar, renomear ou excluir biblioteca. Favoritos são uma lista em memória no `App.vue` (`favoriteTracks`), sem persistência e misturando fontes.
- **Decisão:**
  - **Bibliotecas locais:** catálogo v2 com criar, renomear, excluir e mover faixa. Importar uma pasta cria uma biblioteca com o nome da pasta; arquivos soltos entram na biblioteca escolhida. Tudo é copiado para o domínio do app (pasta gerenciada), com deduplicação por hash.
  - **Exclusão:** excluir uma biblioteca remove só ela e suas referências. O arquivo físico só é apagado quando nenhuma biblioteca nem favorito o usa, em limpeza confirmada que mostra o espaço liberado.
  - **Favoritos:** um flag por faixa, guardado como referência (fonte + id) num arquivo nativo. Global é todas; Local e cada fornecedor são filtros da mesma lista. Favoritar não escreve na conta do YouTube.
- **Consequência:** a interface deixa de ter a lista especial `favorites`; as visões Global, Local e por fornecedor são derivadas. Migração do catálogo v1 para v2 preserva arquivos e faz backup.
- **Revisão:** após a fase 1 (catálogo v2 e armazenamento de favoritos com testes).
