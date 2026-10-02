import * as THREE from "three";
import { EffectComposer } from "three/addons/postprocessing/EffectComposer.js";
import { RenderPass } from "three/addons/postprocessing/RenderPass.js";
import { UnrealBloomPass } from "three/addons/postprocessing/UnrealBloomPass.js";
import { OutputPass } from "three/addons/postprocessing/OutputPass.js";
import { RectAreaLightUniformsLib } from "three/addons/lights/RectAreaLightUniformsLib.js";

// VibeDeck Snake Visualizer
// Normal map: <project-root>/public/snake-normal-map.jpg -> URL /snake-normal-map.jpg
// Curvas e TubeGeometry sao criadas UMA vez. Movimento = shader + offset do normal map.

export const CONFIG = {
  camera: { fov: 42, z: 12.5, near: 0.1, far: 100 },
  geometry: {
    tubularSegments: 80,
    radialSegments: 10,
    controlPoints: 10,
    edgeMargin: 1.5,
    halfWidth: 10.5,
    halfHeight: 6.2,
    curveStrength: 1,
    centerGap: 0,
  },
  texture: {
    repeatX: 3,
    repeatY: 16,
    baseSpeed: 0.025,
    bassSpeed: 0.09,
    energySpeed: 0.035,
  },
  render: { maxPixelRatio: 1.25, exposure: 0.9 },
  bloom: { strength: 0.45, radius: 0.48, threshold: 0.30 },
  layers: [
    { z:  2.0, count: 4, radius: 0.44, scale: 1.05 },
    { z: -0.5, count: 6, radius: 0.38, scale: 1.00 },
    { z: -3.0, count: 8, radius: 0.32, scale: 0.95 },
    { z: -6.0, count: 9, radius: 0.26, scale: 0.90 },
  ],
};

const audio = { subBass: 0, bass: 0, mids: 0, highs: 0, energy: 0, beat: 0 };
const smooth = { ...audio };
const uniforms = {
  uTime: { value: 0 },
  uBass: { value: 0 },
  uSubBass: { value: 0 },
  uEnergy: { value: 0 },
};

export function updateAudioAnalysis(data = {}) {
  for (const key of Object.keys(audio)) {
    audio[key] = THREE.MathUtils.clamp(data[key] ?? 0, 0, 1);
  }
}

function smoothAudio(deltaSeconds) {
  const approach = (current, target, timeConstant) => current + (target - current) * (1 - Math.exp(-deltaSeconds / timeConstant));
  smooth.subBass = approach(smooth.subBass, audio.subBass, 0.085);
  smooth.bass = approach(smooth.bass, audio.bass, 0.085);
  smooth.mids = approach(smooth.mids, audio.mids, 0.085);
  smooth.highs = approach(smooth.highs, audio.highs, 0.085);
  smooth.energy = approach(smooth.energy, audio.energy, 0.205);
  smooth.beat = approach(smooth.beat, audio.beat, 0.051);
}


export function createSnakeVisualizer(container, options = {}) {
  if (!container) throw new Error("Container obrigatorio");

  const cfg = {
    ...CONFIG,
    ...options,
    camera: { ...CONFIG.camera, ...(options.camera || {}) },
    geometry: { ...CONFIG.geometry, ...(options.geometry || {}) },
    texture: { ...CONFIG.texture, ...(options.texture || {}) },
    render: { ...CONFIG.render, ...(options.render || {}) },
    bloom: { ...CONFIG.bloom, ...(options.bloom || {}) },
    layers: options.layers || CONFIG.layers,
  };

  const scene = new THREE.Scene();
  RectAreaLightUniformsLib.init();
  scene.background = null;
  scene.fog = new THREE.FogExp2(0x030507, 0.020);

  const camera = new THREE.PerspectiveCamera(cfg.camera.fov, 1, cfg.camera.near, cfg.camera.far);
  camera.position.set(0, 0, cfg.camera.z);

  const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true, powerPreference: "high-performance" });
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = cfg.render.exposure;
  renderer.setClearColor(0x000000, 0);
  const dpr = () => Math.min(window.devicePixelRatio || 1, cfg.render.maxPixelRatio);
  renderer.setPixelRatio(dpr());
  renderer.domElement.style.cssText = "display:block;width:100%;height:100%";
  container.appendChild(renderer.domElement);

  const composer = new EffectComposer(renderer);
  composer.setPixelRatio(dpr());
  composer.addPass(new RenderPass(scene, camera));
  const bloom = new UnrealBloomPass(new THREE.Vector2(1, 1), cfg.bloom.strength, cfg.bloom.radius, cfg.bloom.threshold);
  composer.addPass(bloom);
  composer.addPass(new OutputPass());

  const baseNormal = new THREE.TextureLoader().load("/snake-normal-map.jpg");
  baseNormal.wrapS = baseNormal.wrapT = THREE.RepeatWrapping;
  baseNormal.repeat.set(cfg.texture.repeatX, cfg.texture.repeatY);
  baseNormal.anisotropy = renderer.capabilities.getMaxAnisotropy();

  // Um material por camada: reduz materiais/texturas e ainda permite offsets diferentes.
  const layerResources = cfg.layers.map((layer, i) => {
    const normalMap = baseNormal.clone();
    normalMap.wrapS = normalMap.wrapT = THREE.RepeatWrapping;
    normalMap.repeat.set(cfg.texture.repeatX, cfg.texture.repeatY);
    normalMap.center.set(0.5, 0.5);
    normalMap.offset.y = (i * 0.173) % 1;
    normalMap.anisotropy = renderer.capabilities.getMaxAnisotropy();
    normalMap.needsUpdate = true;

    const material = new THREE.MeshStandardMaterial({
      color: 0x161719,
      metalness: 0.34,
      roughness: 0.31,
      normalMap,
      normalScale: new THREE.Vector2(1.35, 1.35),
    });

    material.onBeforeCompile = shader => {
      Object.assign(shader.uniforms, uniforms);
      shader.vertexShader = shader.vertexShader.replace(
        "#include <common>",
        `#include <common>\nuniform float uTime;\nuniform float uBass;\nuniform float uSubBass;\nuniform float uEnergy;`
      );
      shader.vertexShader = shader.vertexShader.replace(
        "#include <begin_vertex>",
        `#include <begin_vertex>
        float along = uv.x;
        float waveA = sin(along * 12.0 + position.z * 0.20 - uTime * 1.15);
        float waveB = sin(along * 23.0 + position.y * 0.12 + uTime * 1.70);
        float surfaceWave = waveA * (0.010 + uBass * 0.030) + waveB * (0.004 + uEnergy * 0.010);
        transformed += normal * surfaceWave;
        transformed.x += waveA * (0.018 + uBass * 0.055);
        transformed.y += waveB * (0.010 + uSubBass * 0.030);
        float pulsePos = fract(uTime * 0.16);
        float pulseDist = abs(along - pulsePos);
        pulseDist = min(pulseDist, 1.0 - pulseDist);
        float pulse = exp(-pulseDist * pulseDist * 240.0);
        transformed += normal * pulse * uBass * 0.045;`
      );
    };
    material.customProgramCacheKey = () => "vibedeck-snake-v1";
    return { layer, normalMap, material };
  });
  const textureTransform = { rotation: 0, movement: 1 };
  const geometryShape = { curveAngle: 24, centerGap: 4.8, amplitude: 0.68, thickness: 1.30 };

  // Lighting rig: frente fria revela textura; vermelho fica para centro/rim/back.
  const hemi = new THREE.HemisphereLight(0x40505a, 0x080202, 0.68);
  scene.add(hemi);

  const frontKey = new THREE.RectAreaLight(0xe6edf2, 7.2, 17, 12);
  frontKey.position.set(0, 4, 13); frontKey.lookAt(0, 0, 0); scene.add(frontKey);
  const frontFill = new THREE.RectAreaLight(0x9cabb4, 3.2, 18, 10);
  frontFill.position.set(0, -5, 10); frontFill.lookAt(0, 0, 0); scene.add(frontFill);
  const leftFill = new THREE.RectAreaLight(0x94a9b5, 3.0, 7, 14);
  leftFill.position.set(-10, 1, 7); leftFill.lookAt(0, 0, 0); scene.add(leftFill);
  const rightFill = new THREE.RectAreaLight(0x94a9b5, 3.0, 7, 14);
  rightFill.position.set(10, 1, 7); rightFill.lookAt(0, 0, 0); scene.add(rightFill);

  const backLeft = new THREE.PointLight(0xff1010, 5.5, 21, 1.8);
  backLeft.position.set(-6, 3, -5); scene.add(backLeft);
  const backRight = new THREE.PointLight(0xff1010, 5.5, 21, 1.8);
  backRight.position.set(6, 3, -5); scene.add(backRight);
  const backLower = new THREE.PointLight(0xc00000, 3.2, 18, 2);
  backLower.position.set(0, -6, -5); scene.add(backLower);
  const centerLight = new THREE.PointLight(0xff0808, 5.0, 14, 1.8);
  centerLight.position.set(0, 0, 1); scene.add(centerLight);
  const userLights = [];

  const snakeGroup = new THREE.Group();
  scene.add(snakeGroup);
  const snakes = [];
  // Vertical weave: side lanes rise; center lanes descend. Phase offsets and
  // alternating depth profiles make crossings read as over/under interlacing.
  const DEPTH_PROFILES = [
    [ 0.00,  0.25,  0.70,  1.00,  0.55, -0.20, -0.65, -0.30,  0.10, 0.00 ],
    [ 0.00, -0.30, -0.75, -0.95, -0.40,  0.30,  0.75,  0.35, -0.10, 0.00 ],
    [ 0.00,  0.15,  0.45,  0.15, -0.55, -0.90, -0.35,  0.45,  0.25, 0.00 ],
    [ 0.00, -0.15, -0.45, -0.10,  0.60,  0.90,  0.30, -0.45, -0.25, 0.00 ],
  ];

  let snakeLayout = Array.from({ length: 8 }, (_, i) => ({ lane: i < 2 ? (i ? 'right' : 'left') : 'center', x: [-7.5, 7.5, -2.4, 0, 2.4, -1.2, 1.2, 0][i], z: (i % 2 ? -1 : 1) * (0.35 + (i % 3) * 0.35), scale: 0.82 + (i % 3) * 0.08, phase: i * 0.72 }));
  function createCurve(layer, route, index) {
    const direction = route.lane === 'center' ? -1 : 1;
    const x = route.x;
    const start = new THREE.Vector3(x, direction > 0 ? -cfg.geometry.halfHeight - 1.5 : cfg.geometry.halfHeight + 1.5, layer.z + route.z);
    const end = new THREE.Vector3(x, direction > 0 ? cfg.geometry.halfHeight + 1.5 : -cfg.geometry.halfHeight - 1.5, layer.z + route.z);
    const points = [];
    const count = cfg.geometry.controlPoints;
     const profile = DEPTH_PROFILES[(route.profile ?? index) % DEPTH_PROFILES.length];

    const bend = (1.45 + geometryShape.amplitude * 2.35) * route.bend;
    const phase = index * 0.73;
    const tangentAngle = THREE.MathUtils.degToRad(geometryShape.curveAngle);
    const depthAmount = 0.95 + geometryShape.amplitude * 0.55;

    for (let i = 0; i < count; i++) {
      const t = i / (count - 1);
      const envelope = Math.sin(Math.PI * t);
      const p = new THREE.Vector3().lerpVectors(start, end, t);
      const crossing = Math.sin(t * Math.PI * 2 + route.phase) * (route.lane === 'center' ? 1.05 : 0.32) * geometryShape.amplitude * envelope;
      p.x += crossing;

      // Profundidade artistica real. O perfil e zero nas pontas, preservando a entrada pela borda.
      const pi = Math.min(profile.length - 1, Math.round(t * (profile.length - 1)));
      p.z += profile[pi] * depthAmount * envelope;

      // Mantem uma abertura central limpa, sem esmagar as curvas umas contra as outras.
      const distance = Math.hypot(p.x, p.y);
      if (geometryShape.centerGap > 0 && distance < geometryShape.centerGap) {
        const safeDistance = Math.max(distance, 0.001);
        const push = geometryShape.centerGap - distance;
        p.x += (p.x / safeDistance) * push;
        p.y += (p.y / safeDistance) * push;
      }

      points.push(p);
    }

    if (points.length > 3 && tangentAngle !== 0) { points[1].x += Math.tan(tangentAngle) * 0.18; points[points.length - 2].x -= Math.tan(tangentAngle) * 0.18; }

    return new THREE.CatmullRomCurve3(points, false, "centripetal", 0.35);
  }

  function rebuildSnakes() {
    snakes.forEach(({ mesh, geometry }) => {
      snakeGroup.remove(mesh);
      geometry.dispose();
    });
    snakes.length = 0;

    let globalIndex = 0;
    snakeLayout.slice(0, 8).forEach((route, i) => {
      const layerIndex = i % layerResources.length;
      const layer = cfg.layers[layerIndex];
        const curve = createCurve(layer, route, globalIndex);
        const geometry = new THREE.TubeGeometry(
          curve,
          cfg.geometry.tubularSegments,
          layer.radius * geometryShape.thickness,
          cfg.geometry.radialSegments,
          false
        );
        const mesh = new THREE.Mesh(geometry, layerResources[layerIndex].material);
        mesh.scale.setScalar(layer.scale * route.scale);
        mesh.frustumCulled = true;
        snakeGroup.add(mesh);
        snakes.push({ mesh, geometry, curve, layerIndex, route });
        globalIndex++;
    });
  }
  rebuildSnakes();

  const centerGeo = new THREE.OctahedronGeometry(0.16, 0);
  const centerMat = new THREE.MeshBasicMaterial({ color: 0xff0808 });
  const center = new THREE.Mesh(centerGeo, centerMat);
  center.position.z = 1.25;
  center.rotation.z = Math.PI/4;
  scene.add(center);

  function resize() {
    const w = Math.max(1, container.clientWidth);
    const h = Math.max(1, container.clientHeight);
    camera.aspect = w/h;
    camera.updateProjectionMatrix();
    renderer.setPixelRatio(dpr());
    renderer.setSize(w,h,false);
    composer.setPixelRatio(dpr());
    composer.setSize(w,h);
  }
  const ro = new ResizeObserver(resize);
  ro.observe(container);
  resize();

  const clock = new THREE.Clock();
  let raf = 0;
  let running = true;
  const frameStats = { count: 0, totalMs: 0, lastMs: 0, maxMs: 0 };

  function frame() {
    if (!running) return;
    raf = requestAnimationFrame(frame);
    const frameStartedAt = performance.now();
    const dt = Math.min(clock.getDelta(), 0.05);
    const time = clock.elapsedTime;
    smoothAudio(dt);

    uniforms.uTime.value = time;
    uniforms.uBass.value = smooth.bass;
    uniforms.uSubBass.value = smooth.subBass;
    uniforms.uEnergy.value = smooth.energy;

    const texSpeed = cfg.texture.baseSpeed + smooth.bass*cfg.texture.bassSpeed + smooth.energy*cfg.texture.energySpeed;
    layerResources.forEach((r,i) => {
      r.normalMap.offset.y = (r.normalMap.offset.y + dt*texSpeed*textureTransform.movement*(i%2===0?1:-1)) % 1;
      if (r.normalMap.offset.y < 0) r.normalMap.offset.y += 1;
    });

    frontKey.intensity = 6.6 + smooth.energy*1.1;
    frontFill.intensity = 3.0 + smooth.energy*0.7;
    leftFill.intensity = rightFill.intensity = 2.7 + smooth.mids*1.8;
    backLeft.intensity = backRight.intensity = 4.2 + smooth.bass*9.0;
    backLower.intensity = 2.5 + smooth.subBass*6.0;
    centerLight.intensity = 4.0 + smooth.bass*9.0 + smooth.beat*4.0;
    userLights.forEach(light => { light.intensity = 2.4 + smooth.energy * 1.2; });
    bloom.strength = cfg.bloom.strength + smooth.bass*0.32 + smooth.energy*0.10;

    center.scale.setScalar(1 + smooth.bass*0.45 + smooth.beat*0.30);
    center.rotation.z += dt*(0.10 + smooth.highs*0.15);
    snakeGroup.rotation.z = Math.sin(time*0.055)*0.004;

    composer.render();
    const frameMs = performance.now() - frameStartedAt;
    frameStats.count += 1;
    frameStats.totalMs += frameMs;
    frameStats.lastMs = frameMs;
    frameStats.maxMs = Math.max(frameStats.maxMs, frameMs);
  }
  frame();

  function getStats() {
    const gl = renderer.getContext();
    const ext = gl.getExtension("WEBGL_debug_renderer_info");
    return {
      renderer: ext ? gl.getParameter(ext.UNMASKED_RENDERER_WEBGL) : "indisponivel",
      vendor: ext ? gl.getParameter(ext.UNMASKED_VENDOR_WEBGL) : "indisponivel",
      dpr: renderer.getPixelRatio(),
      canvasWidth: renderer.domElement.width,
      canvasHeight: renderer.domElement.height,
      calls: renderer.info.render.calls,
      triangles: renderer.info.render.triangles,
      geometries: renderer.info.memory.geometries,
      textures: renderer.info.memory.textures,
      snakes: snakes.length,
      frameMs: frameStats.lastMs,
      averageFrameMs: frameStats.count ? frameStats.totalMs / frameStats.count : 0,
      maxFrameMs: frameStats.maxMs,
      frameBudgetMs: 1000 / 60,
    };
  }

  function dispose() {
    running = false;
    cancelAnimationFrame(raf);
    ro.disconnect();
    snakes.forEach(s => s.geometry.dispose());
    layerResources.forEach(r => { r.material.dispose(); r.normalMap.dispose(); });
    baseNormal.dispose();
    centerGeo.dispose(); centerMat.dispose();
    composer.dispose(); renderer.dispose();
    renderer.domElement.remove();
  }

  return {
    scene, camera, renderer, composer, snakes,
    updateAudioAnalysis,
    getStats,
    setNormalMapRepeat(repeatX, repeatY) {
      const x = THREE.MathUtils.clamp(Number(repeatX) || 1, 0.25, 32);
      const y = THREE.MathUtils.clamp(Number(repeatY) || 1, 0.25, 32);
      layerResources.forEach(({ normalMap }) => normalMap.repeat.set(x, y));
    },
    setNormalMapTransform(rotation, movement) {
      textureTransform.rotation = THREE.MathUtils.degToRad(Number(rotation) || 0);
      textureTransform.movement = THREE.MathUtils.clamp(Number(movement) || 0, -6, 6);
      layerResources.forEach(({ normalMap }) => { normalMap.rotation = textureTransform.rotation; });
    },
    setGeometryShape(curveAngle, centerGap, amplitude, thickness) {
      geometryShape.curveAngle = THREE.MathUtils.clamp(Number(curveAngle) || 0, -40, 40);
      geometryShape.centerGap = THREE.MathUtils.clamp(Number(centerGap) || 0, 0, 6);
      geometryShape.amplitude = THREE.MathUtils.clamp(Number(amplitude) || 0, 0, 1.5);
      geometryShape.thickness = THREE.MathUtils.clamp(Number(thickness) || 0, 0.4, 2.5);
      rebuildSnakes();
    },
    setSnakeLayout(layout) {
      if (!Array.isArray(layout)) return;
      snakeLayout = layout.slice(0, 8).map((item, i) => ({ lane: item.lane === 'center' ? 'center' : (i % 2 ? 'right' : 'left'), x: THREE.MathUtils.clamp(Number(item.x) || 0, -9, 9), z: THREE.MathUtils.clamp(Number(item.z) || 0, -6, 3), scale: THREE.MathUtils.clamp(Number(item.scale) || 1, 0.5, 1.4), phase: THREE.MathUtils.clamp(Number(item.phase) || 0, 0, 6.28) }));
      rebuildSnakes();
    },
    setUserLights(layout) {
      if (!Array.isArray(layout)) return;
      userLights.forEach(light => scene.remove(light));
      userLights.length = 0;
      layout.slice(0, 8).forEach(item => {
        const light = new THREE.PointLight(0x9fc8e8, 2.4, 20, 1.6);
        light.position.set(THREE.MathUtils.clamp(Number(item.x) || 0, -12, 12), THREE.MathUtils.clamp(Number(item.y) || 0, -9, 9), THREE.MathUtils.clamp(Number(item.z) || 0, -10, 14));
        light.lookAt(0, 0, 0);
        scene.add(light);
        userLights.push(light);
      });
    },
    dispose,
    setRunning(value) {
      const next = Boolean(value);
      if (next === running) return;
      running = next;
      if (running) { clock.getDelta(); frame(); }
      else { cancelAnimationFrame(raf); raf = 0; }
    },
  };
}

/*
CSS recomendado:

.visualizer-container {
  position: relative;
  overflow: hidden;
  width: 100%;
  height: 100%;
  background: radial-gradient(
    ellipse 65% 75% at 50% 48%,
    #000 0%, #010203 52%, #04080c 76%, #080d12 100%
  );
}

.visualizer-container::after {
  content: "";
  position: absolute;
  inset: 0;
  pointer-events: none;
  background: radial-gradient(
    ellipse 74% 76% at center,
    transparent 0%, transparent 55%,
    rgba(8,13,18,.22) 73%, rgba(8,13,18,.68) 91%, #080d12 100%
  );
}

Uso:
const viz = createSnakeVisualizer(document.querySelector('.visualizer-container'));
viz.updateAudioAnalysis({ subBass:.4, bass:.7, mids:.3, highs:.5, energy:.45, beat:1 });
console.table(viz.getStats());
*/
