// Nerve local dashboard.
// Connects to the same host:port that served this page over WebSocket.

const host = window.location.host || 'localhost:8765';
const tokenInput = document.getElementById('auth-token');
let ws = null;
let sessionId = null;
let history = [];
let lastObservation = null;
let cursorWs = null;
let policy = null;

const el = (id) => document.getElementById(id);
const statusDot = el('status-dot');
const screenshot = el('screenshot');
const cursorPip = el('cursor-pip');

function getToken() {
  return (tokenInput && tokenInput.value && tokenInput.value.trim()) || null;
}

function connect() {
  ws = new WebSocket(`ws://${host}/`);
  ws.addEventListener('open', () => {
    statusDot.classList.remove('bad');
    statusDot.classList.add('ok');
    const token = getToken();
    ws.send(JSON.stringify({
      kind: 'session_start',
      request_id: 'dash-start',
      client_name: 'nerve-dashboard',
      client_version: '0.1.0',
      client_protocol_version: { major: 0, minor: 1, patch: 0 },
      auth_token: token,
    }));
  });

  ws.addEventListener('close', () => {
    statusDot.classList.remove('ok');
    statusDot.classList.add('bad');
    // Reconnect after a short delay.
    setTimeout(connect, 1500);
  });

  ws.addEventListener('error', () => {
    statusDot.classList.remove('ok');
    statusDot.classList.add('bad');
  });

  ws.addEventListener('message', (ev) => {
    let msg;
    try { msg = JSON.parse(ev.data); } catch { return; }
    switch (msg.kind) {
      case 'hello':
        el('platform-pill').textContent = msg.platform;
        if (msg.auth_required && !getToken()) {
          el('auth-banner').classList.remove('hidden');
        } else {
          el('auth-banner').classList.add('hidden');
        }
        break;
      case 'session_started':
        sessionId = msg.session_id;
        el('session-id').textContent = sessionId;
        const caps = msg.capabilities;
        el('backends').textContent = `${caps.backends.screen_capture} · ${caps.backends.input} · ${caps.backends.accessibility}`;
        policy = caps.default_policy || null;
        // Full observations every 500ms.
        ws.send(JSON.stringify({
          kind: 'subscribe_observations',
          request_id: 'dash-sub',
          interval_ms: 500,
          include_screenshot: true,
          delta_frames: true,
        }));
        // Cursor stream at 60Hz for the silky pip.
        ws.send(JSON.stringify({
          kind: 'subscribe_observations',
          request_id: 'dash-cursor',
          interval_ms: 16,
          cursor_only: true,
        }));
        break;
      case 'observation':
        lastObservation = msg.observation;
        renderObservation(msg.observation);
        break;
      case 'cursor_tick':
        renderCursorTick(msg);
        break;
      case 'action_result':
        pushHistory(msg.result);
        break;
      case 'emergency_stopped':
        el('safety-stop').textContent = 'true';
        statusDot.classList.add('bad');
        break;
      case 'error':
        if (msg.code === 'auth_required' || msg.code === 'auth_invalid') {
          el('auth-banner').classList.remove('hidden');
        }
        break;
      default:
        break;
    }
  });
}

function renderObservation(obs) {
  if (obs.screen && obs.screen.screenshot_base64) {
    screenshot.src = `data:image/${obs.screen.screenshot_format};base64,${obs.screen.screenshot_base64}`;
  }
  el('cursor-x').textContent = obs.cursor.x;
  el('cursor-y').textContent = obs.cursor.y;
  el('active-window').textContent = obs.active_window
    ? `${obs.active_window.app_name} · ${obs.active_window.title}`
    : '—';
  el('safety-dry').textContent = obs.safety_state.dry_run;
  el('safety-confirm').textContent = obs.safety_state.confirmation_required;
  el('safety-takeover').textContent = obs.safety_state.human_takeover;
  el('safety-stop').textContent = obs.safety_state.emergency_stopped;
  el('observation-json').textContent = JSON.stringify({
    ...obs,
    screen: { ...obs.screen, screenshot_base64: '<omitted>' },
  }, null, 2);
  renderDirtyTiles(obs);
}

function renderCursorTick(msg) {
  if (!lastObservation) return;
  const obs = lastObservation;
  const w = obs.screen.width || 1;
  const h = obs.screen.height || 1;
  const rect = screenshot.getBoundingClientRect();
  const scaleX = rect.width / w;
  const scaleY = rect.height / h;
  cursorPip.style.left = `${msg.cursor.x * scaleX}px`;
  cursorPip.style.top = `${msg.cursor.y * scaleY}px`;
}

function renderDirtyTiles(obs) {
  const canvas = el('dirty-overlay');
  if (!canvas) return;
  const ctx = canvas.getContext('2d');
  canvas.width = screenshot.clientWidth;
  canvas.height = screenshot.clientHeight;
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  const tiles = obs.dirty_tiles || [];
  if (!tiles.length) return;
  const w = obs.screen.width || 1;
  const h = obs.screen.height || 1;
  const scaleX = canvas.width / w;
  const scaleY = canvas.height / h;
  ctx.strokeStyle = 'rgba(88,197,255,0.5)';
  ctx.lineWidth = 1;
  for (const t of tiles) {
    ctx.strokeRect(t.x * scaleX, t.y * scaleY, t.width * scaleX, t.height * scaleY);
  }
}

function pushHistory(result) {
  history.unshift(result);
  history = history.slice(0, 50);
  const tbody = el('history-body');
  tbody.innerHTML = '';
  for (const r of history) {
    const tr = document.createElement('tr');
    tr.innerHTML = `
      <td>${new Date(r.timestamp).toLocaleTimeString()}</td>
      <td>${r.id}</td>
      <td>${r.method}</td>
      <td style="color:${r.ok ? 'var(--ok)' : 'var(--danger)'}">${r.ok}</td>
    `;
    tbody.appendChild(tr);
  }
  el('last-action').textContent = result.method;
}

if (tokenInput) {
  tokenInput.addEventListener('change', () => {
    if (ws && ws.readyState === WebSocket.OPEN) ws.close();
    connect();
  });
}

el('emergency-stop').addEventListener('click', () => {
  if (!ws || ws.readyState !== WebSocket.OPEN) return;
  ws.send(JSON.stringify({
    kind: 'emergency_stop',
    request_id: 'dash-stop',
  }));
});

const policyForm = document.getElementById('policy-form');
if (policyForm) {
  policyForm.addEventListener('submit', (ev) => {
    ev.preventDefault();
    const dry = document.getElementById('pf-dry').checked;
    const confirm = document.getElementById('pf-confirm').checked;
    const takeover = document.getElementById('pf-takeover').checked;
    const rate = parseInt(document.getElementById('pf-rate').value, 10) || 0;
    const allow = document.getElementById('pf-allow').value
      .split(',').map(s => s.trim()).filter(Boolean);
    const block = document.getElementById('pf-block').value
      .split(',').map(s => s.trim()).filter(Boolean);
    const newPolicy = {
      dry_run: dry,
      require_confirmation: confirm,
      human_takeover: takeover,
      max_actions_per_minute: rate,
      max_session_seconds: 0,
      app_allowlist: allow,
      app_blocklist: block,
      block_password_fields: true,
      confirm_payment_fields: true,
      redact_patterns: [],
    };
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({
        kind: 'set_safety_policy',
        request_id: 'dash-policy',
        policy: newPolicy,
      }));
    }
  });
}

fetch('/api/sessions').then(r => r.json()).then(list => {
  const sel = el('replay-session');
  if (!sel) return;
  for (const s of list) {
    const opt = document.createElement('option');
    opt.value = s;
    opt.textContent = s;
    sel.appendChild(opt);
  }
}).catch(() => {});

const replayBtn = document.getElementById('replay-btn');
const replaySel = el('replay-session');
const replayLog = el('replay-log');
if (replayBtn) {
  replayBtn.addEventListener('click', () => {
    if (!ws || ws.readyState !== WebSocket.OPEN) return;
    if (!replaySel || !replaySel.value) return;
    replayLog.innerHTML = '';
    ws.send(JSON.stringify({
      kind: 'replay_session',
      request_id: 'dash-replay',
      session_id: replaySel.value,
      speed: 4,
    }));
  });
}

connect();
