'use strict';

const $ = (s) => document.querySelector(s);
const EQ_BANDS = [31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];
const HEIGHT_EL = 40;

let S = null;            // latest snapshot from the engine
let draft = null;        // local copy of the active profile (edited optimistically)
let editingUntil = 0;    // while the user edits, don't let server echoes overwrite controls
let telemetry = null;
let profileTimer = null;
let testTimer = null;

const cssVar = (v) => getComputedStyle(document.documentElement).getPropertyValue(v).trim();
const clone = (o) => JSON.parse(JSON.stringify(o));
const fmt = {
  pct: (v) => Math.round(v * 100) + '%',
  deg: (v) => Math.round(v) + '°',
  m: (v) => v.toFixed(1) + ' m',
  s: (v) => v.toFixed(2) + ' s',
  db: (v) => (v > 0 ? '+' : '') + v.toFixed(1) + ' dB',
};

// ---------- server communication ----------

async function send(cmd) {
  try {
    const r = await fetch('/api/cmd', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'X-Aether': '1' },
      body: JSON.stringify(cmd),
    });
    if (!r.ok) { toast(await r.text()); return null; }
    const snap = await r.json();
    applySnapshot(snap);
    return snap;
  } catch {
    toast('Lost connection to Aether.');
    return null;
  }
}

function queueProfileUpdate() {
  editingUntil = Date.now() + 600;
  clearTimeout(profileTimer);
  profileTimer = setTimeout(() => send({ type: 'update_profile', profile: draft }), 70);
}

function queueTest(test) {
  S.test = test;
  clearTimeout(testTimer);
  testTimer = setTimeout(() => send({ type: 'set_test', test }), 40);
}

function connect() {
  const es = new EventSource('/events');
  es.addEventListener('state', (e) => applySnapshot(JSON.parse(e.data)));
  es.addEventListener('telemetry', (e) => { telemetry = JSON.parse(e.data); drawTelemetry(); });
  es.onerror = () => {
    setStatus('Disconnected. Is Aether running?', 'err');
    es.close();
    setTimeout(connect, 1500);
  };
}

let toastTimer = null;
function toast(text) {
  const b = $('#banner');
  b.hidden = false;
  b.textContent = text;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(renderBanner, 5000);
}

// ---------- rendering ----------

function activeProfile() {
  return S.profiles.find((p) => p.id === S.config.profile_id) || S.profiles[0];
}

function applySnapshot(snap) {
  S = snap;
  if (!draft || draft.id !== S.config.profile_id || Date.now() > editingUntil) {
    draft = clone(activeProfile());
  }
  renderTop();
  renderProfiles();
  renderSound();
  renderEq();
  renderTuning();
  renderSettings();
}

function setStatus(text, cls) {
  $('#status-text').textContent = text;
  $('#status').className = 'status ' + (cls || '');
}

function renderTop() {
  const c = S.config;
  document.querySelectorAll('#mode button').forEach((b) => b.classList.toggle('on', b.dataset.v === c.mode));
  $('#enabled').checked = c.enabled;
  $('#enabled-label').textContent = c.enabled ? 'On' : 'Off';
  const st = S.status;
  if (st.error) setStatus(st.error, 'err');
  else if (st.running) {
    const src = st.input ? st.input.replace('Loopback: ', '⟲ ') : 'No source';
    setStatus(`${src} → ${st.output} · ${(st.sample_rate / 1000).toFixed(1)} kHz · ~${Math.round(st.latency_ms)} ms`, 'ok');
  } else setStatus('Starting audio…');
  renderBanner();
}

function renderBanner() {
  const b = $('#banner');
  const st = S && S.status;
  const msg = st && (st.error || st.warning);
  b.hidden = !msg;
  if (!msg) return;
  b.textContent = msg + ' ';
  const a = document.createElement('a');
  a.textContent = 'Setup guide';
  a.onclick = () => { showTab('settings'); $('#guide').open = true; };
  b.appendChild(a);
}

function renderProfiles() {
  const list = $('#profile-list');
  list.innerHTML = '';
  let cat = null;
  for (const p of S.profiles) {
    if (p.category !== cat) {
      cat = p.category;
      const h = document.createElement('div');
      h.className = 'cat';
      h.textContent = cat;
      list.appendChild(h);
    }
    const b = document.createElement('button');
    b.className = 'prof' + (p.id === S.config.profile_id ? ' on' : '');
    b.innerHTML = '<span class="name"></span>';
    b.querySelector('.name').textContent = p.name;
    if (S.modified.includes(p.id)) {
      const d = document.createElement('span');
      d.className = 'mod';
      d.title = 'Modified';
      b.appendChild(d);
    }
    b.title = p.description || p.name;
    b.onclick = () => send({ type: 'select_profile', id: p.id });
    list.appendChild(b);
  }
}

const SOUND_GROUPS = [
  { title: 'Stage', spatial: true, items: [
    ['spatial.upmix', 'Upmix', 0, 1, 0.01, fmt.pct],
    ['spatial.center', 'Center focus', 0, 1, 0.01, fmt.pct],
    ['spatial.front_angle', 'Front width', 15, 60, 1, fmt.deg],
    ['spatial.surround_angle', 'Surround angle', 80, 130, 1, fmt.deg],
    ['spatial.height', 'Height', 0, 1, 0.01, fmt.pct],
    ['spatial.distance', 'Distance', 0.8, 5, 0.1, fmt.m],
  ] },
  { title: 'Room', spatial: true, items: [
    ['spatial.room_size', 'Room size', 0, 1, 0.01, fmt.pct],
    ['spatial.reflections', 'Reflections', 0, 1, 0.01, fmt.pct],
    ['spatial.reverb', 'Reverb', 0, 1, 0.01, fmt.pct],
    ['spatial.decay', 'Decay', 0.1, 3, 0.05, fmt.s],
    ['spatial.damping', 'Damping', 0, 1, 0.01, fmt.pct],
  ] },
  { title: 'Enhance', items: [
    ['tone.bass', 'Bass boost', 0, 1, 0.01, fmt.pct],
    ['tone.dialog', 'Dialog clarity', 0, 1, 0.01, fmt.pct],
    ['tone.detail', 'Detail', 0, 1, 0.01, fmt.pct],
    ['tone.leveler', 'Volume leveler', 0, 1, 0.01, fmt.pct],
    ['tone.preamp', 'Preamp', -24, 12, 0.5, fmt.db],
  ] },
];

const getPath = (o, path) => path.split('.').reduce((a, k) => a[k], o);
const setPath = (o, path, v) => { const ks = path.split('.'); const last = ks.pop(); ks.reduce((a, k) => a[k], o)[last] = v; };

function setFill(input) {
  const pct = ((input.value - input.min) / (input.max - input.min)) * 100;
  input.style.setProperty('--fill', pct + '%');
}

let soundBuilt = false;
const soundInputs = [];

function buildSound() {
  const root = $('#sound-controls');
  for (const g of SOUND_GROUPS) {
    const sec = document.createElement('div');
    sec.className = 'group';
    sec.dataset.spatial = g.spatial ? '1' : '';
    sec.innerHTML = `<h3>${g.title}</h3>`;
    for (const [path, label, min, max, step, f] of g.items) {
      const row = document.createElement('div');
      row.className = 'slider';
      const id = 's-' + path.replace('.', '-');
      row.innerHTML = `<label for="${id}">${label}</label><input type="range" id="${id}" min="${min}" max="${max}" step="${step}"><output></output>`;
      const input = row.querySelector('input');
      const out = row.querySelector('output');
      input.addEventListener('input', () => {
        const v = parseFloat(input.value);
        setPath(draft, path, v);
        out.textContent = f(v);
        setFill(input);
        queueProfileUpdate();
      });
      soundInputs.push({ input, out, path, f });
      sec.appendChild(row);
    }
    root.appendChild(sec);
  }
  soundBuilt = true;
}

function renderSound() {
  if (!soundBuilt) buildSound();
  for (const { input, out, path, f } of soundInputs) {
    const v = getPath(draft, path);
    if (document.activeElement !== input || Date.now() > editingUntil) input.value = v;
    out.textContent = f(v);
    setFill(input);
  }
  const stereo = S.config.mode === 'stereo';
  document.querySelectorAll('#sound-controls .group').forEach((g) => {
    g.style.opacity = g.dataset.spatial && stereo ? 0.45 : 1;
    g.title = g.dataset.spatial && stereo ? 'Spatial settings are bypassed in Stereo mode' : '';
  });
  const nameInput = $('#profile-name');
  if (document.activeElement !== nameInput) nameInput.value = draft.name;
  const modified = S.modified.includes(draft.id);
  $('#profile-badge').textContent = draft.builtin ? (modified ? 'Built-in · modified' : 'Built-in') : 'Custom';
  $('#profile-desc').textContent = draft.description || '';
  const del = $('#delete-profile');
  del.textContent = draft.builtin ? 'Reset' : 'Delete';
  del.disabled = draft.builtin && !modified;
}

// ---------- EQ ----------

function biquadDb(kind, fs, f0, q, gain, f) {
  const A = Math.pow(10, gain / 40);
  const w0 = (2 * Math.PI * Math.min(f0, fs * 0.45)) / fs;
  const cos = Math.cos(w0), alpha = Math.sin(w0) / (2 * Math.max(q, 0.05));
  let b0, b1, b2, a0, a1, a2;
  const sa = 2 * Math.sqrt(A) * alpha;
  switch (kind) {
    case 'low_shelf':
      b0 = A * ((A + 1) - (A - 1) * cos + sa); b1 = 2 * A * ((A - 1) - (A + 1) * cos); b2 = A * ((A + 1) - (A - 1) * cos - sa);
      a0 = (A + 1) + (A - 1) * cos + sa; a1 = -2 * ((A - 1) + (A + 1) * cos); a2 = (A + 1) + (A - 1) * cos - sa; break;
    case 'high_shelf':
      b0 = A * ((A + 1) + (A - 1) * cos + sa); b1 = -2 * A * ((A - 1) + (A + 1) * cos); b2 = A * ((A + 1) + (A - 1) * cos - sa);
      a0 = (A + 1) - (A - 1) * cos + sa; a1 = 2 * ((A - 1) - (A + 1) * cos); a2 = (A + 1) - (A - 1) * cos - sa; break;
    case 'low_pass':
      b0 = (1 - cos) / 2; b1 = 1 - cos; b2 = b0; a0 = 1 + alpha; a1 = -2 * cos; a2 = 1 - alpha; break;
    case 'high_pass':
      b0 = (1 + cos) / 2; b1 = -(1 + cos); b2 = b0; a0 = 1 + alpha; a1 = -2 * cos; a2 = 1 - alpha; break;
    default:
      b0 = 1 + alpha * A; b1 = -2 * cos; b2 = 1 - alpha * A; a0 = 1 + alpha / A; a1 = -2 * cos; a2 = 1 - alpha / A;
  }
  const w = (2 * Math.PI * f) / fs;
  const c1 = Math.cos(w), s1 = Math.sin(w), c2 = Math.cos(2 * w), s2 = Math.sin(2 * w);
  const nr = b0 + b1 * c1 + b2 * c2, ni = -(b1 * s1 + b2 * s2);
  const dr = a0 + a1 * c1 + a2 * c2, di = -(a1 * s1 + a2 * s2);
  return 10 * Math.log10((nr * nr + ni * ni) / (dr * dr + di * di));
}

function fitCanvas(canvas) {
  const dpr = window.devicePixelRatio || 1;
  const w = Math.round(canvas.clientWidth * dpr), h = Math.round(canvas.clientHeight * dpr);
  if (canvas.width !== w || canvas.height !== h) { canvas.width = w; canvas.height = h; }
  const ctx = canvas.getContext('2d');
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  return [ctx, canvas.clientWidth, canvas.clientHeight];
}

function drawCurve(canvas, filters, preamp, range = 15) {
  const [ctx, W, H] = fitCanvas(canvas);
  ctx.clearRect(0, 0, W, H);
  const x = (f) => (Math.log10(f / 20) / 3) * W;
  const y = (db) => H / 2 - (db / range) * (H / 2 - 8);
  ctx.strokeStyle = cssVar('--grid');
  ctx.fillStyle = cssVar('--muted');
  ctx.font = '10px Segoe UI, sans-serif';
  ctx.lineWidth = 1;
  for (const f of [100, 1000, 10000]) {
    ctx.beginPath(); ctx.moveTo(x(f), 0); ctx.lineTo(x(f), H); ctx.stroke();
    ctx.fillText(f >= 1000 ? f / 1000 + 'k' : f, x(f) + 3, H - 4);
  }
  for (const d of [-10, 0, 10]) {
    ctx.beginPath(); ctx.moveTo(0, y(d)); ctx.lineTo(W, y(d)); ctx.stroke();
    ctx.fillText((d > 0 ? '+' : '') + d, 3, y(d) - 3);
  }
  ctx.beginPath();
  for (let i = 0; i <= 200; i++) {
    const f = 20 * Math.pow(1000, i / 200);
    let db = preamp;
    for (const fl of filters) db += biquadDb(fl.kind, 48000, fl.freq, fl.q, fl.gain, f);
    const px = x(f), py = y(Math.max(-range, Math.min(range, db)));
    i ? ctx.lineTo(px, py) : ctx.moveTo(px, py);
  }
  ctx.strokeStyle = cssVar('--accent');
  ctx.lineWidth = 2;
  ctx.stroke();
}

let eqBuilt = false;
const eqInputs = [];

function renderEq() {
  if (!eqBuilt) {
    const root = $('#eq-bands');
    EQ_BANDS.forEach((f, i) => {
      const band = document.createElement('div');
      band.className = 'eq-band';
      band.innerHTML = `<b></b><input type="range" min="-12" max="12" step="0.5" aria-label="${f} Hz"><span>${f >= 1000 ? f / 1000 + 'k' : f}</span>`;
      const input = band.querySelector('input');
      const label = band.querySelector('b');
      input.addEventListener('input', () => {
        draft.tone.eq[i] = parseFloat(input.value);
        label.textContent = draft.tone.eq[i].toFixed(1);
        drawEqCurve();
        queueProfileUpdate();
      });
      eqInputs.push({ input, label });
      root.appendChild(band);
    });
    eqBuilt = true;
  }
  eqInputs.forEach(({ input, label }, i) => {
    if (document.activeElement !== input || Date.now() > editingUntil) input.value = draft.tone.eq[i];
    label.textContent = draft.tone.eq[i].toFixed(1);
  });
  drawEqCurve();
}

function drawEqCurve() {
  const filters = draft.tone.eq.map((g, i) => ({ kind: 'peak', freq: EQ_BANDS[i], q: 1.41, gain: g })).filter((f) => f.gain !== 0);
  drawCurve($('#eq-curve'), filters, 0, 13);
}

// ---------- headphone tuning ----------

function renderTuning() {
  const list = $('#tuning-list');
  list.innerHTML = '';
  for (const t of S.tunings) {
    const row = document.createElement('label');
    row.className = 'opt' + (t.id === S.config.tuning_id ? ' on' : '');
    row.innerHTML = '<input type="radio" name="tuning"><span></span>';
    row.querySelector('span').textContent = t.name;
    const radio = row.querySelector('input');
    radio.checked = t.id === S.config.tuning_id;
    radio.onchange = () => send({ type: 'select_tuning', id: t.id });
    if (!['bypass', 'generic_over_ear', 'generic_on_ear', 'generic_in_ear', 'generic_earbuds'].includes(t.id)) {
      const x = document.createElement('button');
      x.className = 'x';
      x.textContent = '×';
      x.title = 'Remove';
      x.onclick = (e) => { e.preventDefault(); if (confirm(`Remove tuning "${t.name}"?`)) send({ type: 'delete_tuning', id: t.id }); };
      row.appendChild(x);
    }
    list.appendChild(row);
  }
  const active = S.tunings.find((t) => t.id === S.config.tuning_id) || { filters: [], preamp: 0 };
  drawCurve($('#tuning-curve'), active.filters, active.preamp, 15);
}

// ---------- settings ----------

function fillSelect(sel, options, value, autoLabel) {
  sel.innerHTML = '';
  const auto = new Option(autoLabel, '');
  sel.add(auto);
  for (const o of options) sel.add(new Option(`${o.name}  (${o.channels} ch)`, o.name));
  sel.value = value || '';
  if (sel.value !== (value || '')) { sel.add(new Option(value + ' (missing)', value)); sel.value = value; }
}

function renderSettings() {
  const c = S.config;
  document.querySelectorAll('#quality button').forEach((b) => b.classList.toggle('on', b.dataset.v === c.quality));
  fillSelect($('#input-dev'), S.devices.inputs, c.input_device, 'Automatic (virtual cable if found)');
  fillSelect($('#output-dev'), S.devices.outputs, c.output_device, 'Automatic (default device)');
  const g = $('#output-gain');
  if (document.activeElement !== g) g.value = c.output_gain;
  $('#gain-val').textContent = fmt.db(c.output_gain);
  setFill(g);
  $('#autostart').checked = c.autostart;
  const h = $('#test-height');
  if (document.activeElement !== h) h.value = S.test.elevation;
  $('#test-height-val').textContent = Math.round(S.test.elevation) + '°';
  setFill(h);
  $('#test-enabled').checked = S.test.enabled;
}

// ---------- spatial stage ----------

function bedLayout(sp) {
  const fa = Math.min(60, Math.max(15, sp.front_angle));
  const sa = Math.min(130, Math.max(80, sp.surround_angle));
  const ba = Math.min(155, sa + 40);
  return [
    ['L', fa, 0], ['R', -fa, 0], ['C', 0, 0], ['Ls', sa, 0], ['Rs', -sa, 0], ['Lb', ba, 0], ['Rb', -ba, 0],
    ['Ltf', 45, HEIGHT_EL], ['Rtf', -45, HEIGHT_EL], ['Ltr', 135, HEIGHT_EL], ['Rtr', -135, HEIGHT_EL],
  ];
}

let stageGeom = null;
let dragging = false;

function drawStage(t) {
  const canvas = $('#stage');
  const [ctx, W, H] = fitCanvas(canvas);
  ctx.clearRect(0, 0, W, H);
  if (!S || !draft) return;
  const sp = draft.spatial;
  const mode = S.config.mode;
  const enabled = S.config.enabled;
  const depth = 4 + 10 * sp.room_size, width = 3.5 + 8 * sp.room_size;
  const scale = Math.min((W - 30) / width, (H - 30) / depth, 70);
  // Center the room box; the listener sits slightly forward and right of the room's center.
  const cx = W / 2 + 0.03 * width * scale;
  const cy = H / 2 - 0.05 * depth * scale;
  const toPx = (az, el, dist) => {
    const r = dist * Math.cos((el * Math.PI) / 180) * scale;
    const a = (az * Math.PI) / 180;
    return [cx - Math.sin(a) * r, cy - Math.cos(a) * r];
  };
  stageGeom = { cx, cy, scale };

  // Room outline (listener sits 0.55 of the depth from the back wall, 0.47 of the width from the right wall).
  const left = cx - 0.53 * width * scale, top = cy - 0.45 * depth * scale;
  ctx.fillStyle = cssVar('--surface-2');
  ctx.strokeStyle = cssVar('--border');
  ctx.lineWidth = 1.5;
  ctx.beginPath();
  ctx.roundRect(left, top, width * scale, depth * scale, 8);
  ctx.fill();
  ctx.stroke();
  ctx.fillStyle = cssVar('--muted');
  ctx.font = '11px Segoe UI, sans-serif';
  ctx.fillText(`Room ${width.toFixed(1)} × ${depth.toFixed(1)} m`, left + 8, top + 16);

  const accent = cssVar('--accent');
  const dist = sp.distance;
  // Distance ring.
  ctx.setLineDash([4, 5]);
  ctx.strokeStyle = cssVar('--grid');
  ctx.beginPath();
  ctx.arc(cx, cy, dist * scale, 0, Math.PI * 2);
  ctx.stroke();
  ctx.setLineDash([]);

  const spatialOn = enabled && mode !== 'stereo';
  for (const [name, az, el] of bedLayout(sp)) {
    const visible = spatialOn || name === 'L' || name === 'R';
    const [x, y] = toPx(az, el, dist);
    const height = el > 0;
    ctx.globalAlpha = visible ? 1 : 0.18;
    ctx.beginPath();
    ctx.arc(x, y, height ? 7 : 10, 0, Math.PI * 2);
    if (height) {
      ctx.strokeStyle = cssVar('--accent-2');
      ctx.lineWidth = 2;
      ctx.stroke();
    } else {
      ctx.fillStyle = spatialOn ? accent : cssVar('--muted');
      ctx.fill();
    }
    // Ear-level labels sit outside the ring, height labels inside it, so they never collide.
    const a = (az * Math.PI) / 180;
    const off = height ? -17 : 22;
    ctx.fillStyle = height ? cssVar('--accent-2') : cssVar('--text');
    ctx.font = (height ? '600 9.5px' : '600 11px') + ' Segoe UI, sans-serif';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(name, x - Math.sin(a) * off, y - Math.cos(a) * off);
    ctx.textBaseline = 'alphabetic';
    ctx.globalAlpha = 1;
  }
  ctx.textAlign = 'left';

  // Listener.
  ctx.fillStyle = cssVar('--text');
  ctx.beginPath(); ctx.arc(cx, cy, 13, 0, Math.PI * 2); ctx.fill();
  ctx.beginPath(); ctx.moveTo(cx - 5, cy - 12); ctx.lineTo(cx, cy - 20); ctx.lineTo(cx + 5, cy - 12); ctx.fill();
  ctx.fillRect(cx - 16, cy - 4, 4, 8); ctx.fillRect(cx + 12, cy - 4, 4, 8);

  if (mode === 'speakers' && enabled) {
    ctx.fillStyle = cssVar('--muted');
    for (const s of [-1, 1]) ctx.fillRect(cx + s * 34 - 6, cy - 46, 12, 9);
  }

  // Test sound.
  const test = S.test;
  const [bx, by] = toPx(test.azimuth, test.elevation, dist * 0.92);
  const lift = test.elevation / 90;
  const radius = 9 + lift * 6;
  if (test.enabled) {
    const phase = (t / 500) % 1;
    ctx.strokeStyle = accent;
    ctx.globalAlpha = 1 - phase;
    ctx.lineWidth = 2;
    ctx.beginPath(); ctx.arc(bx, by, radius + phase * 22, 0, Math.PI * 2); ctx.stroke();
    ctx.globalAlpha = 1;
  }
  const grad = ctx.createRadialGradient(bx - 3, by - 3, 1, bx, by, radius);
  grad.addColorStop(0, '#ffffff');
  grad.addColorStop(1, test.enabled ? '#fbbf24' : cssVar('--muted'));
  ctx.fillStyle = grad;
  ctx.shadowColor = test.enabled ? '#fbbf24' : 'transparent';
  ctx.shadowBlur = test.enabled ? 18 : 0;
  ctx.beginPath(); ctx.arc(bx, by, radius, 0, Math.PI * 2); ctx.fill();
  ctx.shadowBlur = 0;

  if (!enabled) {
    ctx.fillStyle = cssVar('--muted');
    ctx.font = '13px Segoe UI, sans-serif';
    ctx.textAlign = 'center';
    ctx.fillText('Processing is off', cx, H - 14);
    ctx.textAlign = 'left';
  } else if (mode === 'stereo') {
    ctx.fillStyle = cssVar('--muted');
    ctx.font = '13px Segoe UI, sans-serif';
    ctx.textAlign = 'center';
    ctx.fillText('Stereo mode: virtualization off, enhancements on', cx, H - 14);
    ctx.textAlign = 'left';
  }
}

function stagePointer(e) {
  if (!stageGeom || !S) return;
  const rect = $('#stage').getBoundingClientRect();
  const dx = e.clientX - rect.left - stageGeom.cx;
  const dy = e.clientY - rect.top - stageGeom.cy;
  const az = (Math.atan2(-dx, -dy) * 180) / Math.PI;
  queueTest({ ...S.test, enabled: true, azimuth: Math.round(az) });
}

function frame(t) {
  drawStage(t);
  requestAnimationFrame(frame);
}

// ---------- telemetry ----------

function drawTelemetry() {
  if (!telemetry) return;
  const [ctx, W, H] = fitCanvas($('#spectrum'));
  ctx.clearRect(0, 0, W, H);
  const bands = telemetry.spectrum;
  const n = bands.length;
  const bw = W / n;
  const grad = ctx.createLinearGradient(0, H, 0, 0);
  grad.addColorStop(0, cssVar('--accent'));
  grad.addColorStop(1, cssVar('--accent-2'));
  ctx.strokeStyle = cssVar('--grid');
  for (const d of [-20, -40, -60]) {
    const y = H * (-d / 80);
    ctx.beginPath(); ctx.moveTo(0, y); ctx.lineTo(W, y); ctx.stroke();
  }
  ctx.fillStyle = grad;
  for (let i = 0; i < n; i++) {
    const v = Math.max(0, Math.min(1, (bands[i] + 80) / 80));
    const h = v * H;
    ctx.beginPath();
    ctx.roundRect(i * bw + 1, H - h, Math.max(1, bw - 2), h, 2);
    ctx.fill();
  }
  ctx.fillStyle = cssVar('--muted');
  ctx.font = '10px Segoe UI, sans-serif';
  for (const [f, label] of [[100, '100'], [1000, '1k'], [10000, '10k']]) {
    ctx.fillText(label, (Math.log10(f / 20) / 3) * W + 2, H - 3);
  }
  const m = (id, db) => $(id).style.setProperty('--v', Math.max(0, Math.min(1, (db + 60) / 60)));
  m('#m-in-l', telemetry.in[0]); m('#m-in-r', telemetry.in[1]);
  m('#m-out-l', telemetry.out[0]); m('#m-out-r', telemetry.out[1]);
  $('#load').textContent = `DSP ${(telemetry.load * 100).toFixed(1)}%`;
}

// ---------- events ----------

function showTab(name) {
  document.querySelectorAll('.tabs button').forEach((b) => b.classList.toggle('active', b.dataset.tab === name));
  document.querySelectorAll('.tab').forEach((t) => (t.hidden = t.id !== 'tab-' + name));
  if (S) { drawEqCurve(); renderTuning(); }
}

function download(name, text) {
  const a = document.createElement('a');
  a.href = URL.createObjectURL(new Blob([text], { type: 'application/json' }));
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
}

function wire() {
  document.querySelectorAll('#mode button').forEach((b) => (b.onclick = () => send({ type: 'set_mode', mode: b.dataset.v })));
  document.querySelectorAll('#quality button').forEach((b) => (b.onclick = () => send({ type: 'set_quality', quality: b.dataset.v })));
  document.querySelectorAll('.tabs button').forEach((b) => (b.onclick = () => showTab(b.dataset.tab)));
  $('#enabled').onchange = (e) => send({ type: 'set_enabled', enabled: e.target.checked });

  const newProfile = () => {
    const name = prompt('Name for the new profile', draft.name + ' (custom)');
    if (name) send({ type: 'create_profile', name, from: draft.id });
  };
  $('#new-profile').onclick = newProfile;
  $('#dup-profile').onclick = newProfile;
  $('#export-profile').onclick = () => {
    const { id, builtin, ...rest } = draft;
    download(draft.name.replace(/[^\w\- ]+/g, '').trim() + '.aether.json', JSON.stringify(rest, null, 2));
  };
  $('#import-profile').onclick = () => $('#profile-file').click();
  $('#profile-file').onchange = async (e) => {
    const f = e.target.files[0];
    if (!f) return;
    try {
      const p = JSON.parse(await f.text());
      if (!p.spatial || !p.tone) throw new Error();
      send({ type: 'import_profile', profile: { ...p, id: '', builtin: false } });
    } catch { toast('That file is not an Aether profile.'); }
    e.target.value = '';
  };
  $('#delete-profile').onclick = () => {
    if (draft.builtin) send({ type: 'delete_profile', id: draft.id });
    else if (confirm(`Delete profile "${draft.name}"?`)) send({ type: 'delete_profile', id: draft.id });
  };
  $('#profile-name').onchange = (e) => {
    draft.name = e.target.value;
    queueProfileUpdate();
  };
  $('#eq-reset').onclick = () => {
    draft.tone.eq = draft.tone.eq.map(() => 0);
    renderEq();
    queueProfileUpdate();
  };

  $('#tuning-file-btn').onclick = () => $('#tuning-file').click();
  $('#tuning-file').onchange = async (e) => {
    const f = e.target.files[0];
    if (!f) return;
    $('#tuning-text').value = await f.text();
    if (!$('#tuning-name').value) $('#tuning-name').value = f.name.replace(/ParametricEQ\.txt$|\.txt$/i, '').trim();
    e.target.value = '';
  };
  $('#tuning-import').onclick = async () => {
    const snap = await send({ type: 'import_tuning', name: $('#tuning-name').value, text: $('#tuning-text').value });
    if (snap) { $('#tuning-text').value = ''; $('#tuning-name').value = ''; }
  };

  const setDevices = () => send({ type: 'set_devices', input: $('#input-dev').value || null, output: $('#output-dev').value || null });
  $('#input-dev').onchange = setDevices;
  $('#output-dev').onchange = setDevices;
  $('#refresh-dev').onclick = () => send({ type: 'refresh_devices' });
  $('#output-gain').oninput = (e) => {
    setFill(e.target);
    $('#gain-val').textContent = fmt.db(parseFloat(e.target.value));
    clearTimeout(profileTimer);
    profileTimer = setTimeout(() => send({ type: 'set_output_gain', db: parseFloat(e.target.value) }), 80);
  };
  $('#autostart').onchange = (e) => send({ type: 'set_autostart', enabled: e.target.checked });
  $('#quit').onclick = () => { if (confirm('Quit Aether? System audio will stop being processed.')) send({ type: 'quit' }).then(() => window.close()); };

  $('#test-enabled').onchange = (e) => queueTest({ ...S.test, enabled: e.target.checked });
  $('#test-height').oninput = (e) => {
    setFill(e.target);
    $('#test-height-val').textContent = e.target.value + '°';
    queueTest({ ...S.test, elevation: parseFloat(e.target.value) });
  };

  const stage = $('#stage');
  stage.addEventListener('pointerdown', (e) => { dragging = true; stage.classList.add('dragging'); stage.setPointerCapture(e.pointerId); stagePointer(e); });
  stage.addEventListener('pointermove', (e) => dragging && stagePointer(e));
  const end = () => { dragging = false; stage.classList.remove('dragging'); };
  stage.addEventListener('pointerup', end);
  stage.addEventListener('pointercancel', end);
  window.addEventListener('resize', () => S && (drawEqCurve(), renderTuning()));
}

wire();
fetch('/api/state').then((r) => r.json()).then(applySnapshot).catch(() => setStatus('Cannot reach Aether', 'err'));
connect();
requestAnimationFrame(frame);
