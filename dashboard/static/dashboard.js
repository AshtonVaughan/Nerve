// Nerve local dashboard.
// Connects to the same host:port that served this page over WebSocket.

const host = window.location.host || 'localhost:8765';
const ws = new WebSocket(`ws://${host}/`);
let sessionId = null;
let history = [];

const el = (id) => document.getElementById(id);
const statusDot = el('status-dot');
const screenshot = el('screenshot');
const cursorPip = el('cursor-pip');

ws.addEventListener('open', () => {
  statusDot.classList.add('ok');
  ws.send(JSON.stringify({
    kind: 'session_start',
    request_id: 'dash-start',
    client_name: 'nerve-dashboard',
    client_version: '0.1.0',
  }));
});

ws.addEventListener('close', () => {
  statusDot.classList.remove('ok');
  statusDot.classList.add('bad');
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
      break;
    case 'session_started':
      sessionId = msg.session_id;
      el('session-id').textContent = sessionId;
      const caps = msg.capabilities;
      el('backends').textContent = `${caps.backends.screen_capture} · ${caps.backends.input} · ${caps.backends.accessibility}`;
      ws.send(JSON.stringify({
        kind: 'subscribe_observations',
        request_id: 'dash-sub',
        interval_ms: 500,
        include_screenshot: true,
      }));
      break;
    case 'observation':
      renderObservation(msg.observation);
      break;
    case 'action_result':
      pushHistory(msg.result);
      break;
    case 'emergency_stopped':
      el('safety-stop').textContent = 'true';
      statusDot.classList.add('bad');
      break;
    default:
      break;
  }
});

function renderObservation(obs) {
  if (obs.screen && obs.screen.screenshot_base64) {
    screenshot.src = `data:image/${obs.screen.screenshot_format};base64,${obs.screen.screenshot_base64}`;
    const w = obs.screen.width || 1;
    const h = obs.screen.height || 1;
    const rect = screenshot.getBoundingClientRect();
    const scaleX = rect.width / w;
    const scaleY = rect.height / h;
    cursorPip.style.left = `${obs.cursor.x * scaleX}px`;
    cursorPip.style.top = `${obs.cursor.y * scaleY}px`;
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

el('emergency-stop').addEventListener('click', () => {
  ws.send(JSON.stringify({
    kind: 'emergency_stop',
    request_id: 'dash-stop',
  }));
});

fetch('/api/sessions').then(r => r.json()).then(list => {
  const sel = el('replay-session');
  for (const s of list) {
    const opt = document.createElement('option');
    opt.value = s;
    opt.textContent = s;
    sel.appendChild(opt);
  }
}).catch(() => {});
