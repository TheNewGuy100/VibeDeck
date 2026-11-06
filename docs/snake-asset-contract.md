# Contrato do modelo das cobras (Blender → app)

Regras para o `.glb` exportado do Blender. Validadas com o tubo de teste (`sandbox/snake-test.glb`) na página `sandbox/snake-shader.html` (`npm run dev`, abrir `/sandbox/snake-shader.html`). Estado: contrato v1, o importador do app ainda não existe.

## Objetos e propriedades

Tudo que o app deve ler fica em `VD_Export`, sob um único `VD_Root` (`vd_schema = 1`). O que não tem prefixo `VD_` não é exportado nem lido (câmera de prévia, moldura, luzes reais de prévia ficam em `Preview_Only`).

| `vd_kind` | Objeto | Propriedades |
|---|---|---|
| `root` | `VD_Root` | `vd_schema`, `vd_aspect`, `vd_light_unit` |
| `bounds` | `VD_Bounds` | `vd_half_width`, `vd_half_height` (área visível a profundidade 0) |
| `snake_body` | malha | `vd_layer`, `vd_phase`, `vd_speed`, `vd_clip` |
| `snake_rig` | armature | `vd_for` (id da cobra) |
| `light` | vazio | `vd_light` (`rect`/`point`), `vd_intensity`, `vd_color`, `vd_width`, `vd_height`, `vd_audio_react` |
| `bar` | vazio | `vd_band`, `vd_axis`, `vd_gain` |
| `anchor` | vazio | ponto nomeado |

`vd_id` é único. A luz real do Blender fica só como prévia, filha do vazio, fora de `VD_Export`.

## Malha da cobra

- **UV `UVMap` (contrato):** U em volta do corpo (0 a 1, sem emenda: a última faixa chega a 1, não volta a 0), V ao longo do corpo, **V = 0 na cabeça e V = 1 na cauda**. O exportador glTF grava `1 − V`; o shader desfaz isso.
- **UV `UV_normal`:** coordenadas do normal map já com a troca de eixos, repetição 0,75 × 5 e rotação de 91° do app. Sai como `TEXCOORD_1`.
- Eixo do tubo no espaço de repouso: X (o shader recebe `uAxis`). Raio medido da própria malha.
- Pontas fechadas e arredondadas. Anéis: o tubo de teste usa 192 anéis × 16 lados (3 mil vértices); com isso as dobras e o inchaço ficam suaves.
- Material: PBR com cor `#161719`, metal 0,34, rugosidade 0,31, normal map `public/snake-normal-map.jpg` com intensidade 1,35.

## Rig e animação

- Hierarquia: `root` (fixo na origem, ponto de encaixe) → `anchor` (recebe só translação) → `spine_00` … `spine_23` (só rotação).
- 24 ossos de corpo, com a cabeça em `spine_00`. Ossos "dobráveis" (B-Bones) não são exportados pelo glTF, por isso a suavidade vem de mais ossos.
- Pesos lineares entre ossos vizinhos, por posição ao longo do corpo.
- Clipes de 48 quadros que repetem (último quadro igual ao primeiro): `sine_loop` (onda sem ponto fixo; o `anchor` mantém o centro parado), `slither`, `idle`.
- Não animar o `root`. Mover a cobra é mover o `root`.
- Aviso aberto: a malha é irmã do rig sob `VD_Root`, não filha. O exportador avisa. Funciona porque ambos estão na origem.

## Shader (src/visualizers/snake-shader-patch.ts, snake-phase.ts)

- **Uma só fase** (0 a 1), integrada em JS: `fase += dt · velocidade / período`. Corpo (clipe), fluxo da textura e pulsos leem a mesma fase. O áudio nunca define a fase, só a velocidade e o inchaço.
- **Fluxo da textura:** deslocamento do normal map em **ladrilhos inteiros por volta** (`uScroll = sentido · sinal_da_inclinação · ladrilhos`). Com número inteiro de ladrilhos, a fase 1 equivale à fase 0. O sinal da inclinação é medido na geometria (`normalUvSlopeSign`), então a "frente" vem do UV e não de uma rotação fixa.
- **Bass:** velocidade `1 + ganho·envelope`, limitada a [0,6; 2]. Inchaço radial em espaço de repouso, antes do skinning, mais pulsos que nascem na cabeça e correm até a cauda (até 4).
- **Limite do inchaço:** `safeSwellLimit(raio, menor raio de curvatura)`. Para o tubo de teste (raio 0,18, curvatura mínima ~0,7) o teto geométrico é alto; o limite prático é o do controle (padrão +12%).

## Validação (tubo de teste, Windows, navegador embutido, 2026-11-05)

- Loop: fase 0 contra fase 1 difere em 1 de 3,7 milhões de valores de cor, por 3 de 255 (ruído de ponto flutuante). Tolerância adotada: no máximo 1 em 100 000 valores, nenhum acima de 8. Repetir a fase 0 dá diferença zero. Entre fases distintas, 89 mil valores mudam.
- Fluxo: 2 ladrilhos por volta equivalem a 0,4 comprimento de corpo por volta, na direção cabeça → cauda.
- Inchaço de 12% aumenta a área coberta da cobra em ~8% (medida em pixels, aproximada).
- Testes: 15 novos (`snake-phase.test.ts`, `snake-shader-patch.test.ts`); suíte completa 194 passando; `vue-tsc` limpo.

Pendente: ligar ao importador e ao áudio real; ver o movimento rodando de olho; o teste de inchaço mede cobertura em pixels, não colisão entre dobras; só o tubo de teste foi usado, e o corpo final pode pedir outros valores de escala e amplitude.
