// ─── Shared UI components ─────────────────────────────────────────────────────
// All functions return HTML strings. Call reinitIcons() after injecting them
// into the DOM so Lucide can stamp the <i data-lucide="..."> elements.
// Depends on: js/utils.js  (esc)

// ── Badge ────────────────────────────────────────────────────────────────────

const BADGE_STYLES = {
  green:  'background:#052e16;color:#4ade80',
  red:    'background:#450a0a;color:#f87171',
  yellow: 'background:#422006;color:#fbbf24',
  blue:   'background:#0c1a3d;color:#60a5fa',
  purple: 'background:#2e1065;color:#c084fc',
  gray:   'background:#1f2937;color:#9ca3af',
};

function badge(text, variant = 'gray') {
  const s = BADGE_STYLES[variant] || BADGE_STYLES.gray;
  return `<span style="${s};display:inline-flex;align-items:center;padding:2px 8px;border-radius:9999px;font-size:12px;font-weight:500">${esc(text)}</span>`;
}

// ── Loading / error states ───────────────────────────────────────────────────

function spinner() {
  return `<div style="display:flex;justify-content:center;align-items:center;padding:64px">
    <div class="spin" style="width:32px;height:32px;border:3px solid #374151;border-top-color:#60a5fa;border-radius:50%"></div>
  </div>`;
}

function emptyState(msg = 'No items found') {
  return `<div style="display:flex;flex-direction:column;align-items:center;padding:48px;color:#4b5563">
    <svg width="36" height="36" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" style="margin-bottom:10px">
      <circle cx="12" cy="12" r="10"/><path d="M12 8v4"/><path d="M12 16h.01"/>
    </svg>
    <p style="font-size:14px">${esc(msg)}</p>
  </div>`;
}

function errBanner(msg) {
  return `<div style="margin:24px;padding:12px 16px;border:1px solid #7f1d1d;border-radius:8px;background:#450a0a;color:#fca5a5;font-size:14px">
    ⚠ ${esc(msg)}
  </div>`;
}

// ── Page chrome ──────────────────────────────────────────────────────────────

/**
 * Standard page header with title, optional subtitle, and an actions slot.
 * @param {string} title
 * @param {string} [subtitle]
 * @param {string} [actions]  — pre-built HTML string (e.g. refreshBtn(...))
 */
function pageHeader(title, subtitle = '', actions = '') {
  return `<div style="border-bottom:1px solid #1f2937;padding:20px 24px 18px">
    <div style="display:flex;align-items:flex-start;justify-content:space-between;gap:16px">
      <div>
        <h1 style="font-size:20px;font-weight:600;color:#fff;margin:0">${esc(title)}</h1>
        ${subtitle ? `<p style="font-size:13px;color:#6b7280;margin:4px 0 0">${esc(subtitle)}</p>` : ''}
      </div>
      <div style="flex-shrink:0">${actions}</div>
    </div>
  </div>`;
}

/**
 * Refresh button that calls a named global function when clicked.
 * @param {string} fnName — name of the global function to call, e.g. 'loadS3'
 */
function refreshBtn(fnName) {
  return `<button onclick="${fnName}()" style="display:inline-flex;align-items:center;gap:6px;padding:6px 14px;border-radius:6px;font-size:13px;background:#1f2937;border:1px solid #374151;color:#d1d5db;cursor:pointer" onmouseenter="this.style.background='#374151'" onmouseleave="this.style.background='#1f2937'">
    <i data-lucide="refresh-cw" style="width:13px;height:13px"></i> Refresh
  </button>`;
}

// ── Stat card ────────────────────────────────────────────────────────────────

function statCard(label, value, color = '#60a5fa') {
  return `<div style="background:#111827;border:1px solid #1f2937;border-radius:8px;padding:16px 20px">
    <p style="font-size:12px;color:#6b7280;font-weight:500;margin:0 0 4px">${esc(label)}</p>
    <p style="font-size:26px;font-weight:700;color:${color};margin:0">${esc(String(value))}</p>
  </div>`;
}

// ── Resource table ───────────────────────────────────────────────────────────

/**
 * Generic data table.
 * @param {Array<{header: string, render: (row) => string}>} columns
 * @param {Array<object>} rows
 */
function tableHtml(columns, rows) {
  if (!rows || rows.length === 0) return emptyState();

  const th = columns.map(c =>
    `<th style="padding:8px 14px;text-align:left;font-size:11px;font-weight:600;text-transform:uppercase;letter-spacing:0.05em;color:#6b7280;border-bottom:1px solid #1f2937;white-space:nowrap">${esc(c.header)}</th>`
  ).join('');

  const tbody = rows.map(r => {
    const tds = columns.map(c =>
      `<td style="padding:11px 14px;font-size:13px;color:#d1d5db;border-bottom:1px solid #111827;overflow:hidden;max-width:320px;text-overflow:ellipsis">${c.render(r)}</td>`
    ).join('');
    return `<tr onmouseenter="this.style.background='#1a2234'" onmouseleave="this.style.background=''">${tds}</tr>`;
  }).join('');

  return `<div style="overflow-x:auto"><table style="width:100%;border-collapse:collapse"><thead><tr>${th}</tr></thead><tbody>${tbody}</tbody></table></div>`;
}

// ── Section card ─────────────────────────────────────────────────────────────

/** Titled section card wrapping arbitrary content HTML. */
function card(title, content) {
  return `<div style="margin-bottom:24px">
    <h2 style="font-size:12px;font-weight:600;color:#9ca3af;margin:0 0 10px;text-transform:uppercase;letter-spacing:0.04em">${esc(title)}</h2>
    <div style="background:#111827;border:1px solid #1f2937;border-radius:8px;overflow:hidden">${content}</div>
  </div>`;
}

// ── Inline helpers ───────────────────────────────────────────────────────────

/** Monospace code span. */
function mono(s) {
  return `<code style="font-family:ui-monospace,SFMono-Regular,monospace;font-size:12px;color:#9ca3af">${esc(s)}</code>`;
}
