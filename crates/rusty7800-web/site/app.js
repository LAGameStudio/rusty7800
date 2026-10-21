// Rusty7800 web shell: owns the frame loop around the wasm core.
// Build: `wasm-pack build crates/rusty7800-web --target web --out-dir site/pkg`
// then EITHER serve `site/` over HTTP (`python3 site/serve.py`; opening
// index.html from disk fails — browsers block modules + wasm fetch on
// file:// with CORS errors) OR open the bundled single file
// `site/rusty7800-standalone.html` (`python3 site/bundle.py` to rebuild;
// no server, no fetch, works from file://).
import init, { Emu, machine_list, machine_labels, machine_is_pal, wasm_memory } from './pkg/rusty7800_web.js';

const $ = (id) => document.getElementById(id);
const canvas = $('screen');
const ctx = canvas.getContext('2d');
let emu = null;
let running = false;
let frames = 0;
// Epoch: every fresh rAF chain captures it; a stale queued callback from
// before a machine/ROM switch sees a mismatch and exits instead of
// double-stepping the new session.
let runId = 0;
// Kept so a machine switch reloads the same ROM (like the desktop GUI)
// instead of dropping it — the mismatch warning then clears live.
let romBytes = null;
let romName = '';

// Keyboard state -> port input. P1: arrows + Z (A) / X (B), matching the
// desktop GUI. P2: WASD + G (A) / H (B). Console: T reset, E select,
// O pause (MAME INPUT_PORTS defaults in ctrl/console.rs).
const keys = new Set();
function pushInput() {
  if (!emu) return;
  emu.set_p1(
    keys.has('ArrowUp'), keys.has('ArrowDown'),
    keys.has('ArrowLeft'), keys.has('ArrowRight'),
    keys.has('KeyZ'), keys.has('KeyX'),
  );
  emu.set_p2(
    keys.has('KeyW'), keys.has('KeyS'),
    keys.has('KeyA'), keys.has('KeyD'),
    keys.has('KeyG'), keys.has('KeyH'),
  );
  emu.set_console(keys.has('KeyT'), keys.has('KeyE'), keys.has('KeyO'));
}
window.addEventListener('keydown', (ev) => {
  if (ev.code === 'Tab') return; // let focus move
  if (['ArrowUp','ArrowDown','ArrowLeft','ArrowRight','Space'].includes(ev.code)) {
    ev.preventDefault(); // don't scroll the page mid-game
  }
  if (ev.repeat) return;
  keys.add(ev.code);
  pushInput();
});
window.addEventListener('keyup', (ev) => {
  keys.delete(ev.code);
  pushInput();
});
window.addEventListener('blur', () => {
  keys.clear();
  pushInput(); // stuck-key safety: release everything on focus loss
});

// ---- Audio: core renders mono i16 at 31400 Hz (TIA_AUDIO_CLOCK); the
// device runs at whatever it runs at. This mirrors the desktop Pump
// (linear resample, silence on underrun, 2 s backlog cap). ScriptProcessor
// is deprecated but keeps the standalone single-file bundle working with
// no extra worklet file; a 4k buffer is plenty at 60 fps.
let actx = null;
let audioQueue = []; // f32 mono @ emu rate
let audioPhase = 0;
let audioMuted = false;
function ensureAudio() {
  if (actx) {
    if (actx.state === 'suspended') actx.resume();
    return;
  }
  const AC = window.AudioContext || window.webkitAudioContext;
  if (!AC) return;
  actx = new AC();
  const size = 4096;
  const node = actx.createScriptProcessor(size, 0, 1);
  node.onaudioprocess = (ev) => {
    const out = ev.outputBuffer.getChannelData(0);
    const inRate = (emu && emu.audio_rate()) || 31400;
    const step = inRate / actx.sampleRate;
    for (let i = 0; i < out.length; i++) {
      const idx = Math.floor(audioPhase);
      if (idx + 1 >= audioQueue.length) {
        out[i] = 0; // underrun: hold position, emit silence
        continue;
      }
      const frac = audioPhase - idx;
      out[i] = audioQueue[idx] * (1 - frac) + audioQueue[idx + 1] * frac;
      audioPhase += step;
    }
    const consumed = Math.min(Math.floor(audioPhase), Math.max(0, audioQueue.length - 1));
    if (consumed > 0) {
      audioQueue.splice(0, consumed);
      audioPhase -= consumed;
    }
  };
  node.connect(actx.destination);
  if (actx.state === 'suspended') actx.resume();
}
function pushFrameAudio() {
  if (!emu || !actx || audioMuted) {
    // Drain so an unplayed backlog can't burst out later.
    if (emu) emu.take_audio();
    return;
  }
  const s = emu.take_audio(); // Int16Array view (copied)
  for (let i = 0; i < s.length; i++) audioQueue.push(s[i] / 32768);
  const cap = 2 * ((emu && emu.audio_rate()) || 31400);
  if (audioQueue.length > cap) {
    const drop = audioQueue.length - cap;
    audioQueue.splice(0, drop);
    audioPhase = Math.max(0, audioPhase - drop);
  }
}

async function boot() {
  await init();
  const names = machine_list();
  const labels = machine_labels();
  names.forEach((name, i) => {
    const opt = document.createElement('option');
    opt.value = name;
    opt.textContent = labels[i] || name;
    $('machine').appendChild(opt);
  });
  newEmu();
}

function newEmu() {
  runId += 1; // kill any in-flight frame chain for the old session
  emu = new Emu($('machine').value);
  frames = 0;
  running = false;
  keys.clear();
  pushInput();
  if (romBytes) {
    const slot = emu.load(romBytes);
    blit();
    $('status').textContent = `${romName} slot=${slot}`;
    appendRegion();
    running = true;
    startLoop();
  } else {
    $('status').textContent = 'pick a ROM';
  }
  updateTvWarn();
}

function appendRegion() {
  const region = typeof emu.rom_is_pal === 'function'
    ? (emu.rom_is_pal() === undefined ? '' : ` tv=${emu.rom_is_pal() ? 'PAL' : 'NTSC'}`)
    : '';
  $('status').textContent += region;
}

// ROM-region vs machine-region warning. PAL-timed games hang on a
// 263-line machine (Fatal Run PAL re-boots forever on `a7800`), so say
// so loudly and name the fix.
function updateTvWarn() {
  const el = $('tvwarn');
  if (!el) return;
  el.textContent = '';
  if (!emu || typeof emu.rom_is_pal !== 'function') return;
  const romPal = emu.rom_is_pal();
  if (romPal === undefined) return; // headerless: no region claim
  const mPal = machine_is_pal($('machine').value);
  if (mPal === undefined || romPal === mPal) return;
  const want = romPal ? 'a7800p' : 'a7800';
  el.textContent =
    ` REGION MISMATCH: ${romPal ? 'PAL' : 'NTSC'} ROM on ` +
    `${mPal ? 'PAL' : 'NTSC'} machine — switch Machine to ${want}.`;
}

function blit() {
  emu.update_rgba();
  const w = emu.width(), h = emu.height();
  if (canvas.width !== w || canvas.height !== h) {
    canvas.width = w;
    canvas.height = h;
  }
  // Zero-copy view into wasm linear memory (fresh each frame: growth
  // detaches old buffers). Copied once into the ImageData for blitting.
  const mem = new Uint8ClampedArray(wasm_memory().buffer, emu.rgba_ptr(), emu.rgba_len());
  const img = new ImageData(mem.slice(), w, h);
  ctx.putImageData(img, 0, 0);
  const s = Math.max(1, Math.min(4, parseInt($('scale').value, 10) || 2));
  canvas.style.width = `${w * s}px`;
  canvas.style.height = `${h * s}px`;
  $('status').textContent =
    `frame ${frames} pc=$${emu.pc().toString(16).padStart(4, '0')} hash=${emu.frame_hash().toString(16)}`;
}

function loop(myId) {
  if (!running || myId !== runId) return;
  emu.step_frames(1);
  pushFrameAudio();
  frames += 1;
  blit();
  requestAnimationFrame(() => loop(myId));
}

/// Start one frame chain; supersedes any earlier chain (stale callbacks
/// exit on the epoch check instead of stepping the new machine).
function startLoop() {
  runId += 1;
  const id = runId;
  requestAnimationFrame(() => loop(id));
}

$('machine').addEventListener('change', () => { newEmu(); });
$('scale').addEventListener('change', () => { if (emu) blit(); });

$('rom').addEventListener('change', async (ev) => {
  const file = ev.target.files[0];
  if (!file) return;
  ensureAudio(); // user gesture: unlock the device
  romBytes = null; // newEmu must not resurrect the previous ROM here
  romName = '';
  newEmu();
  const bytes = new Uint8Array(await file.arrayBuffer());
  romBytes = bytes;
  romName = file.name;
  const slot = emu.load(bytes);
  frames = 0;
  blit();
  $('status').textContent += ` slot=${slot}`;
  appendRegion();
  updateTvWarn();
  running = true;
  startLoop();
});

$('run').addEventListener('click', () => {
  if (!emu) return;
  ensureAudio(); // user gesture: unlock the device
  running = !running;
  $('run').textContent = running ? 'Stop' : 'Run';
  if (running) startLoop();
});
$('pause').addEventListener('click', () => {
  running = false;
  $('run').textContent = 'Run';
});
$('step').addEventListener('click', () => {
  if (!emu) return;
  running = false;
  $('run').textContent = 'Run';
  emu.step_frames(1);
  pushFrameAudio();
  frames += 1;
  blit();
});
$('mute').addEventListener('click', () => {
  audioMuted = !audioMuted;
  $('mute').textContent = audioMuted ? 'Unmute' : 'Mute';
});

boot();
