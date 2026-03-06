// ─── String / format utilities ───────────────────────────────────────────────

/** Escape a value for safe insertion into innerHTML. */
function esc(v) {
  const d = document.createElement('div');
  d.textContent = (v == null ? '' : String(v));
  return d.innerHTML;
}

function fmtDate(s) {
  if (!s) return '—';
  try { return new Date(s).toLocaleString(); } catch { return s; }
}

function fmtBytes(n) {
  const b = Number(n);
  if (isNaN(b)) return '—';
  if (b < 1024) return `${b} B`;
  if (b < 1048576) return `${(b / 1024).toFixed(1)} KB`;
  return `${(b / 1048576).toFixed(1)} MB`;
}

// ─── Polling helpers ─────────────────────────────────────────────────────────

const _pollers = {};

function startPoll(key, fn, ms) {
  stopPoll(key);
  fn();
  _pollers[key] = setInterval(fn, ms);
}

function stopPoll(key) {
  if (_pollers[key]) { clearInterval(_pollers[key]); delete _pollers[key]; }
}

function stopAllPolls() {
  Object.keys(_pollers).forEach(stopPoll);
}

// ─── Lucide icon refresh ─────────────────────────────────────────────────────

/** Call after any innerHTML update that injects <i data-lucide="..."> elements. */
function reinitIcons() {
  if (window.lucide) lucide.createIcons();
}
