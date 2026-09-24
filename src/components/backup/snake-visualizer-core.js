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

function smoothAudio() {
  smooth.subBass += (audio.subBass - smooth.subBass) * 0.18;
  smooth.bass += (audio.bass - smooth.bass) * 0.18;
  smooth.mids += (audio.mids - smooth.mids) * 0.18;
  smooth.highs += (audio.highs - smooth.highs) * 0.18;
  smooth.energy += (audio.energy - smooth.energy) * 0.08;
  smooth.beat += (audio.beat - smooth.beat) * 0.28;
}

function mulberry32(seed) {
  return () => {
    let t = (seed += 0x6d2b79f5);
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
const rand = (rng, min, max) => min + (max - min) * rng();

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
  scene.fog = new THREE.FogExp2(0x030507, 0.026);

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
      color: 0x121212,
      metalness: 0.28,
      roughness: 0.27,
      normalMap,
      normalScale: new THREE.Vector2(1.15, 1.15),
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
  const geometryShape = { curveAngle: 30, centerGap: 5, endpointCenter: -0.4, amplitude: 0.7, minEndpointDistance: 22.25, thickness: 1.8, curvesPerEdge: 2 };

  // Lighting rig: frente fria revela textura; vermelho fica para centro/rim/back.
  const hemi = new THREE.HemisphereLight(0x35404a, 0x080202, 0.55);
  scene.add(hemi);

  const frontKey = new THREE.RectAreaLight(0xdce4e8, 6, 16, 12);
  frontKey.position.set(0, 4, 13); frontKey.lookAt(0, 0, 0); scene.add(frontKey);
  const frontFill = new THREE.RectAreaLight(0x8b9ba5, 2.8, 18, 10);
  frontFill.position.set(0, -5, 10); frontFill.lookAt(0, 0, 0); scene.add(frontFill);
  const leftFill = new THREE.RectAreaLight(0x8fa3ad, 2.8, 7, 14);
  leftFill.position.set(-10, 1, 7); leftFill.lookAt(0, 0, 0); scene.add(leftFill);
  const rightFill = new THREE.RectAreaLight(0x8fa3ad, 2.8, 7, 14);
  rightFill.position.set(10, 1, 7); rightFill.lookAt(0, 0, 0); scene.add(rightFill);

  const backLeft = new THREE.PointLight(0xff0808, 7, 20, 1.8);
  backLeft.position.set(-6, 3, -5); scene.add(backLeft);
  const backRight = new THREE.PointLight(0xff0808, 7, 20, 1.8);
  backRight.position.set(6, 3, -5); scene.add(backRight);
  const backLower = new THREE.PointLight(0xb00000, 4, 18, 2);
  backLower.position.set(0, -6, -5); scene.add(backLower);
  const centerLight = new THREE.PointLight(0xff0505, 8, 13, 1.7);
  centerLight.position.set(0, 0, 1); scene.add(centerLight);

  const snakeGroup = new THREE.Group();
  scene.add(snakeGroup);
  const snakes = [];
  const occupiedPaths = [];
  const edges = ["left", "right", "top", "bottom"];
  const anchors = [
    [-5,3],[0,4],[5,3],[-6,0],[6,0],[-5,-3],[0,-4],[5,-3]
  ].map(([x,y]) => new THREE.Vector2(x,y));

  function edgePoint(edge, z, rng) {
    const { halfWidth:w, halfHeight:h, edgeMargin:m } = cfg.geometry;
    const proximity = geometryShape.endpointCenter;
    const edgeY = () => rand(rng,-h,h) * (1 - proximity);
    const edgeX = () => rand(rng,-w,w) * (1 - proximity);
    if (edge === "left") return new THREE.Vector3(-w-m, edgeY(), z);
    if (edge === "right") return new THREE.Vector3(w+m, edgeY(), z);
    if (edge === "top") return new THREE.Vector3(edgeX(), h+m, z);
    return new THREE.Vector3(edgeX(), -h-m, z);
  }

  function edgeDistance(edge, first, second) {
    return edge === "left" || edge === "right"
      ? Math.abs(first.y - second.y)
      : Math.abs(first.x - second.x);
  }

  function edgePair(edgeIndex) {
    const a = edgeIndex % edges.length;
    // Local arches are more readable than a single line crossing the whole scene.
    // Each endpoint is still sampled independently, so every snake gets a different span.
    return [edges[a], edges[a]];
  }

  function createCurve(layer, index, rng, edgeIndex) {
    const [a,b] = edgePair(edgeIndex);
    const localArc = a === b;
    let start = edgePoint(a, layer.z, rng);
    let end = edgePoint(b, layer.z, rng);
    let attempts = 0;
    while (edgeDistance(a, start, end) < geometryShape.minEndpointDistance && attempts < 48) {
      end = edgePoint(b, layer.z, rng);
      attempts++;
    }
    const anchor = anchors[index % anchors.length];
    const curveStrength = Math.abs(geometryShape.curveAngle) / 40 * geometryShape.amplitude;
    const curveDirection = Math.sign(geometryShape.curveAngle) || (rng() > 0.5 ? 1 : -1);
    const curveVariation = rand(rng, 0.72, 1.28);
    const curvePhase = rand(rng, 0, Math.PI * 2);
    const waveFreqX = rand(rng, 2.2, 3.8);
    const waveFreqY = rand(rng, 2.5, 4.4);
    const waveAmpX = rand(rng, 0.8, 2.4);
    const waveAmpY = rand(rng, 0.7, 2.0);
    // Keep depth variation subtle so TubeGeometry does not twist its Frenet frames abruptly.
    const waveAmpZ = rand(rng, 0.05, 0.25);
    const baselineXY = new THREE.Vector2(end.x - start.x, end.y - start.y);
    const arcNormal = baselineXY.lengthSq() > 0.001
      ? new THREE.Vector2(-baselineXY.y, baselineXY.x).normalize()
      : new THREE.Vector2(1, 0);
    const toCenter = new THREE.Vector2(-(start.x + end.x) * 0.5, -(start.y + end.y) * 0.5);
    const centerSide = toCenter.dot(arcNormal);
    const arcSide = Math.abs(centerSide) > 0.001 ? Math.sign(centerSide) : (rng() > 0.5 ? 1 : -1);
    const arcWidth = localArc ? (1.2 + geometryShape.amplitude * 1.8) * curveVariation * arcSide : 0;
    const points = [];

    for (let i=0; i<cfg.geometry.controlPoints; i++) {
      const t = i/(cfg.geometry.controlPoints-1);
      const p = new THREE.Vector3().lerpVectors(start,end,t);
      if (i !== 0 && i !== cfg.geometry.controlPoints-1) {
        const envelope = Math.sin(t*Math.PI);
        const pullRange = localArc
          ? [0.05 + curveStrength * 0.55, Math.min(0.95, 0.1 + curveStrength * 0.75)]
          : [0.15, 0.34];
        const pull = envelope * rand(rng, pullRange[0], pullRange[1]) * curveVariation;
        p.x = THREE.MathUtils.lerp(p.x, anchor.x, pull);
        p.y = THREE.MathUtils.lerp(p.y, anchor.y, pull);
        p.x += Math.sin(t*Math.PI*waveFreqX+index*0.83+curvePhase)*waveAmpX*envelope*curveStrength*curveVariation*curveDirection;
        p.y += Math.cos(t*Math.PI*waveFreqY+index*0.57+curvePhase)*waveAmpY*envelope*curveStrength*curveVariation;
        p.z += Math.sin(t*Math.PI*2+index*0.71+curvePhase)*waveAmpZ*envelope*curveStrength*curveVariation;
        p.x += arcNormal.x * Math.sin(t * Math.PI) * arcWidth;
        p.y += arcNormal.y * Math.sin(t * Math.PI) * arcWidth;
        const distance = Math.hypot(p.x, p.y);
        if (geometryShape.centerGap > 0 && distance < geometryShape.centerGap) {
          const angle = distance > 0.001 ? Math.atan2(p.y, p.x) : Math.atan2(anchor.y, anchor.x);
          p.x = Math.cos(angle) * geometryShape.centerGap;
          p.y = Math.sin(angle) * geometryShape.centerGap;
        }
      }
      points.push(p);
    }
    // Mirror paired offsets so both ends of one local arc keep the same curvature.
    for (let i = 1; i < points.length - 1; i++) {
      const mirroredIndex = points.length - 1 - i;
      if (i >= mirroredIndex) break;
      const t = i / (points.length - 1);
      const mirroredT = mirroredIndex / (points.length - 1);
      const base = new THREE.Vector3().lerpVectors(start, end, t);
      const mirroredBase = new THREE.Vector3().lerpVectors(start, end, mirroredT);
      const offset = points[i].clone().sub(base).add(points[mirroredIndex].clone().sub(mirroredBase)).multiplyScalar(0.5);
      points[i].copy(base).add(offset);
      points[mirroredIndex].copy(mirroredBase).add(offset);
    }
    // Smooth the control polygon before interpolation so bends gain distance instead of becoming corners.
    for (let pass = 0; pass < 6; pass++) {
      for (let i = 1; i < points.length - 1; i++) {
        const midpoint = points[i - 1].clone().add(points[i + 1]).multiplyScalar(0.5);
        points[i].lerp(midpoint, 0.55);
      }
    }
    // Treat the curve angle as mirrored endpoint tangents around the Z axis.
    // The snake opens by +theta at the start and closes by -theta at the end.
    if (points.length > 3 && geometryShape.curveAngle !== 0 && end.distanceToSquared(start) > 0.001) {
      const baseline = end.clone().sub(start);
      const tangentLength = baseline.length() / (points.length - 1) * 1.7;
      const axis = new THREE.Vector3(0, 0, 1);
      const angle = THREE.MathUtils.degToRad(geometryShape.curveAngle);
      const startTangent = baseline.clone().normalize().applyAxisAngle(axis, angle);
      const endTangent = baseline.clone().normalize().applyAxisAngle(axis, -angle);
      points[1].copy(start).add(startTangent.multiplyScalar(tangentLength));
      points[points.length - 2].copy(end).sub(endTangent.multiplyScalar(tangentLength));
    }
    // Keep intermediate turns gradual; otherwise the spline can form hooks between samples.
    const maxTurn = THREE.MathUtils.degToRad(20);
    for (let pass = 0; pass < 12; pass++) {
      for (let i = 1; i < points.length - 1; i++) {
        const incoming = points[i].clone().sub(points[i - 1]).normalize();
        const outgoing = points[i + 1].clone().sub(points[i]).normalize();
        const turn = incoming.angleTo(outgoing);
        if (turn > maxTurn) {
          const midpoint = points[i - 1].clone().add(points[i + 1]).multiplyScalar(0.5);
          points[i].lerp(midpoint, THREE.MathUtils.clamp((turn - maxTurn) / turn * 0.75, 0.15, 0.75));
        }
      }
    }
    return new THREE.CatmullRomCurve3(points, false, "centripetal", 0.35);
  }

  function rebuildSnakes() {
    snakes.forEach(({ mesh, geometry }) => { snakeGroup.remove(mesh); geometry.dispose(); });
    snakes.length = 0;
    occupiedPaths.length = 0;
    let globalIndex = 0;
    cfg.layers.forEach((layer, li) => {
      const layerCount = geometryShape.curvesPerEdge * 4;
      for (let i=0; i<layerCount; i++) {
        let curve;
        let samples;
        let bestCollision = -Infinity;
        for (let attempt = 0; attempt < 12; attempt++) {
          const rng = mulberry32(0xBEEF + li*1009 + i*7919 + attempt*104729);
          const candidate = createCurve(layer, globalIndex, rng, i);
          const candidateSamples = candidate.getPoints(24);
          let nearest = Infinity;
          for (const occupied of occupiedPaths) {
            if (occupied.layerIndex !== li) continue;
            for (const point of candidateSamples) {
              for (const occupiedPoint of occupied.samples) {
                nearest = Math.min(nearest, point.distanceTo(occupiedPoint) - layer.radius - occupied.radius);
              }
            }
          }
          if (nearest >= 0.3) {
            curve = candidate;
            samples = candidateSamples;
            break;
          }
          if (nearest > bestCollision) {
            curve = candidate;
            samples = candidateSamples;
            bestCollision = nearest;
          }
        }
        if (!curve || !samples) {
          const rng = mulberry32(0xBEEF + li*1009 + i*7919);
          curve = createCurve(layer, globalIndex, rng, i);
          samples = curve.getPoints(24);
        }
        occupiedPaths.push({ layerIndex: li, samples, radius: layer.radius * geometryShape.thickness });
        const geometry = new THREE.TubeGeometry(curve, cfg.geometry.tubularSegments, layer.radius * geometryShape.thickness, cfg.geometry.radialSegments, false);
        const mesh = new THREE.Mesh(geometry, layerResources[li].material);
        mesh.scale.setScalar(layer.scale);
        snakeGroup.add(mesh);
        snakes.push({ mesh, geometry, curve, layerIndex: li });
        globalIndex++;
      }
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

  function frame() {
    if (!running) return;
    raf = requestAnimationFrame(frame);
    const dt = Math.min(clock.getDelta(), 0.05);
    const time = clock.elapsedTime;
    smoothAudio();

    uniforms.uTime.value = time;
    uniforms.uBass.value = smooth.bass;
    uniforms.uSubBass.value = smooth.subBass;
    uniforms.uEnergy.value = smooth.energy;

    const texSpeed = cfg.texture.baseSpeed + smooth.bass*cfg.texture.bassSpeed + smooth.energy*cfg.texture.energySpeed;
    layerResources.forEach((r,i) => {
      r.normalMap.offset.y = (r.normalMap.offset.y + dt*texSpeed*textureTransform.movement*(i%2===0?1:-1)) % 1;
      if (r.normalMap.offset.y < 0) r.normalMap.offset.y += 1;
    });

    frontKey.intensity = 5.5 + smooth.energy*1.0;
    frontFill.intensity = 2.5 + smooth.energy*0.7;
    leftFill.intensity = rightFill.intensity = 2.4 + smooth.mids*2.2;
    backLeft.intensity = backRight.intensity = 5 + smooth.bass*12;
    backLower.intensity = 3 + smooth.subBass*8;
    centerLight.intensity = 6 + smooth.bass*14 + smooth.beat*8;
    bloom.strength = cfg.bloom.strength + smooth.bass*0.45 + smooth.energy*0.12;

    center.scale.setScalar(1 + smooth.bass*0.45 + smooth.beat*0.30);
    center.rotation.z += dt*(0.10 + smooth.highs*0.15);
    snakeGroup.rotation.z = Math.sin(time*0.055)*0.004;

    composer.render();
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
    setGeometryShape(curveAngle, centerGap, endpointCenter, amplitude, minEndpointDistance, thickness, curvesPerEdge) {
      geometryShape.curveAngle = THREE.MathUtils.clamp(Number(curveAngle) || 0, -40, 40);
      geometryShape.centerGap = THREE.MathUtils.clamp(Number(centerGap) || 0, 0, 5);
      geometryShape.endpointCenter = THREE.MathUtils.clamp(Number(endpointCenter) || 0, -4, 1);
      geometryShape.amplitude = THREE.MathUtils.clamp(Number(amplitude) || 0, 0, 1.5);
      geometryShape.minEndpointDistance = THREE.MathUtils.clamp(Number(minEndpointDistance) || 0, 0, 36);
      geometryShape.thickness = THREE.MathUtils.clamp(Number(thickness) || 0, 0.4, 2.5);
      geometryShape.curvesPerEdge = Math.round(THREE.MathUtils.clamp(Number(curvesPerEdge) || 1, 1, 8));
      rebuildSnakes();
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
