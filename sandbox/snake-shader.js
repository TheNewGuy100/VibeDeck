// Dev-only sandbox: the snake exported from Blender, with the production shader patch and the phase
// and bass logic from src/visualizers. No audio and no app session: bass is simulated by controls.
import * as THREE from 'three';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import {
  BassEnvelope,
  PhaseClock,
  PulseTrain,
  safeSwellLimit,
  speedFromBass,
  swellFromBass,
} from '/src/visualizers/snake-phase.ts';
import {
  MAX_PULSES,
  SNAKE_PROGRAM_KEY,
  applySnakeShader,
  createSnakeUniforms,
  normalUvSlopeSign,
} from '/src/visualizers/snake-shader-patch.ts';

const $ = (id) => document.getElementById(id);
const canvas = $('view');
const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, preserveDrawingBuffer: true });
renderer.setPixelRatio(1);
renderer.setClearColor(0x0a0a0b);
const scene = new THREE.Scene();
const camera = new THREE.PerspectiveCamera(42, 1, 0.1, 100); // same lens and distance as the app
camera.position.set(0, 0, 12.5);
const controls = new OrbitControls(camera, canvas);
controls.enableDamping = false;

scene.add(new THREE.HemisphereLight(0x40505a, 0x080202, 0.9));
const key = new THREE.DirectionalLight(0xbfd4ff, 2.2); key.position.set(0, 4, 13); scene.add(key);
const rimA = new THREE.DirectionalLight(0xff3b30, 1.6); rimA.position.set(-6, 3, -5); scene.add(rimA);
const rimB = new THREE.DirectionalLight(0xff3b30, 1.2); rimB.position.set(6, -3, -5); scene.add(rimB);

const params = { playing: true, tiles: 2, flow: 1, speedGain: 0.6, swellGain: 0.12, pulseAmp: 0.08, bass: 0, auto: true };
const uniforms = createSnakeUniforms();
const clock = new PhaseClock(6);
const envelope = new BassEnvelope();
const pulses = new PulseTrain(MAX_PULSES, 1.4);

let mixer = null;
let actions = {};
let activeClip = 'sine_loop';
let duration = 1;
let slopeSign = 1;
let restRadius = 0.18;
let safeLimit = 0.12;
let skeletonHelper = null;
let mesh = null;
let beatClock = 0;
let lastBeatKick = 0;

function resize() {
  const w = window.innerWidth, h = window.innerHeight;
  renderer.setSize(w, h, false);
  camera.aspect = w / h; camera.updateProjectionMatrix();
}
window.addEventListener('resize', resize); resize();

function bindRange(id, onInput) {
  const el = $(id), out = el.parentElement.querySelector('output');
  const apply = () => { if (out) out.textContent = Number(el.value).toFixed(el.step.includes('.') ? 2 : 0); onInput(Number(el.value)); };
  el.addEventListener('input', apply); apply();
}

/** Poses the body and sets the shader uniforms for one phase value (raw, not wrapped). */
function drawAt(phase) {
  if (mixer) mixer.setTime(phase * duration);
  uniforms.uPhase.value = phase;
  uniforms.uScroll.value = params.flow * slopeSign * params.tiles;
  renderer.render(scene, camera);
}

function applyBassToUniforms(speedValue) {
  const swell = Math.min(swellFromBass(envelope.value, params.swellGain, 0.4), safeLimit);
  uniforms.uSwell.value = swell;
  for (let i = 0; i < MAX_PULSES; i += 1) {
    const pulse = pulses.pulses[i];
    const amp = pulse ? Math.min(pulse.amplitude * params.pulseAmp, Math.max(0, safeLimit - swell)) : 0;
    uniforms.uPulses.value[i].set(pulse ? pulse.position : 0, amp);
  }
  return speedValue;
}

let lastTime = performance.now();
function frame(now) {
  requestAnimationFrame(frame);
  const dt = Math.min(0.1, (now - lastTime) / 1000); lastTime = now;
  let raw = params.bass;
  if (params.auto) {
    beatClock += dt;
    if (beatClock >= 0.5) { beatClock -= 0.5; lastBeatKick = 0; pulses.spawn(1); }
    lastBeatKick += dt;
    raw = Math.max(raw, Math.max(0, 1 - lastBeatKick / 0.12));
  }
  envelope.update(raw, dt);
  const speed = speedFromBass(envelope.value, params.speedGain);
  if (params.playing) { clock.advance(dt, speed); $('phase').value = clock.phase; $('phase').dispatchEvent(new Event('input')); }
  pulses.update(dt, speed);
  applyBassToUniforms(speed);
  drawAt(clock.phase);
  $('status').textContent =
    `fase ${clock.phase.toFixed(3)}   velocidade ${speed.toFixed(2)}x\n` +
    `bass ${envelope.value.toFixed(2)}   inchaço ${(uniforms.uSwell.value * 100).toFixed(1)}% (limite ${(safeLimit * 100).toFixed(0)}%)\n` +
    `sentido do relevo: ${params.flow * slopeSign * params.tiles >= 0 ? '+' : '-'}${Math.abs(params.tiles)} ladrilhos/volta`;
}

/** Reads the canvas after rendering a given phase with the audio effects switched off. */
function capture(phase) {
  const saved = { swell: uniforms.uSwell.value, pulses: uniforms.uPulses.value.map((p) => p.clone()) };
  uniforms.uSwell.value = 0; uniforms.uPulses.value.forEach((p) => p.set(0, 0));
  drawAt(phase);
  const gl = renderer.getContext();
  const w = gl.drawingBufferWidth, h = gl.drawingBufferHeight;
  const pixels = new Uint8Array(w * h * 4);
  gl.readPixels(0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
  uniforms.uSwell.value = saved.swell; uniforms.uPulses.value.forEach((p, i) => p.copy(saved.pulses[i]));
  return pixels;
}
const diff = (a, b) => { let max = 0, count = 0; for (let i = 0; i < a.length; i += 1) { const d = Math.abs(a[i] - b[i]); if (d > max) max = d; if (d > 2) count += 1; } return { max, count, total: a.length }; };

function loopTest() {
  const wasPlaying = params.playing; params.playing = false;
  const at0 = capture(0), at1 = capture(1), mid = capture(0.37), at0b = capture(0);
  const resumePhase = clock.phase;
  drawAt(resumePhase);
  params.playing = wasPlaying;
  const loop = diff(at0, at1), repeat = diff(at0, at0b), moved = diff(at0, mid);
  // Phase 1 samples the texture at a whole-tile shift of phase 0, so float rounding may move a
  // handful of colour values by a few levels. Tolerance: at most 1 in 100 000 values, none above 8/255.
  const ok = loop.count <= loop.total * 1e-5 && loop.max <= 8 && repeat.count === 0 && moved.count > 0;
  return { ok, loop_phase0_vs_phase1: loop, deterministic_phase0_twice: repeat, moves_between_phases: moved,
    flow: { slopeSign, towardTail: params.flow === 1, scrollTiles: uniforms.uScroll.value } };
}

function showTest() {
  const result = loopTest();
  const el = $('result');
  el.className = result.ok ? 'ok' : 'bad';
  el.textContent = `loop ${result.ok ? 'fecha' : 'NÃO fecha'}: fase 0 vs 1 → ${result.loop_phase0_vs_phase1.count} px diferentes (máx ${result.loop_phase0_vs_phase1.max}); `
    + `fase 0 vs 0,37 → ${result.moves_between_phases.count} px (tem que ser > 0)`;
  return result;
}

new GLTFLoader().load('/sandbox/snake-test.glb', (gltf) => {
  scene.add(gltf.scene);
  gltf.scene.traverse((obj) => { if (obj.isSkinnedMesh) mesh = obj; });
  const material = mesh.material;
  material.onBeforeCompile = (shader) => applySnakeShader(shader, uniforms);
  material.customProgramCacheKey = () => SNAKE_PROGRAM_KEY;
  material.needsUpdate = true;
  slopeSign = normalUvSlopeSign(mesh.geometry) || 1;

  const position = mesh.geometry.getAttribute('position');
  restRadius = 0;
  for (let i = 0; i < position.count; i += 1) restRadius = Math.max(restRadius, Math.hypot(position.getY(i), position.getZ(i)));
  safeLimit = safeSwellLimit(restRadius, 0.7); // tightest bend of the sine clip is ~0.7

  mixer = new THREE.AnimationMixer(gltf.scene);
  gltf.animations.forEach((clip) => { actions[clip.name] = mixer.clipAction(clip); });
  const select = $('clip');
  Object.keys(actions).forEach((name) => select.add(new Option(name, name)));
  const pick = (name) => { Object.values(actions).forEach((a) => a.stop()); activeClip = name; actions[name].play(); duration = actions[name].getClip().duration; };
  select.addEventListener('change', () => pick(select.value));
  select.value = 'sine_loop'; pick('sine_loop');

  skeletonHelper = new THREE.SkeletonHelper(gltf.scene); skeletonHelper.visible = false; scene.add(skeletonHelper);
  $('bones').addEventListener('change', (e) => { skeletonHelper.visible = e.target.checked; });
  $('wire').addEventListener('change', (e) => { material.wireframe = e.target.checked; });

  window.__snake = {
    ready: true, loopTest, capture, drawAt, params, uniforms, mesh, clock, envelope, pulses,
    info: () => ({ clip: activeClip, duration, slopeSign, restRadius, safeLimit, bones: mesh.skeleton.bones.length, vertices: mesh.geometry.getAttribute('position').count }),
  };
  requestAnimationFrame(frame);
});

$('play').addEventListener('click', (e) => { params.playing = !params.playing; e.target.textContent = params.playing ? 'Pausar' : 'Reproduzir'; });
$('beat').addEventListener('click', () => { pulses.spawn(1); lastBeatKick = 0; beatClock = 0; });
$('test').addEventListener('click', showTest);
$('flow').addEventListener('change', (e) => { params.flow = Number(e.target.value); });
$('auto').addEventListener('change', (e) => { params.auto = e.target.checked; });
bindRange('period', (v) => { clock.periodSeconds = v; });
bindRange('phase', (v) => { if (!params.playing) clock.phase = v; });
bindRange('tiles', (v) => { params.tiles = Math.round(v); });
bindRange('bass', (v) => { params.bass = v; });
bindRange('speedGain', (v) => { params.speedGain = v; });
bindRange('swellGain', (v) => { params.swellGain = v; });
bindRange('pulseAmp', (v) => { params.pulseAmp = v; });
