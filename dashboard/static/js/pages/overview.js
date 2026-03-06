// ─── Page: Overview ──────────────────────────────────────────────────────────
// Depends on: api, components (pageHeader, statCard, card, badge, refreshBtn, errBanner), utils (reinitIcons)

async function loadOverview() {
  const el = document.getElementById('page-overview');
  try {
    const d = await api.health();
    const entries = Object.entries(d.services || {});
    const ok = entries.filter(([, v]) => v).length;

    const serviceRows = entries.map(([name, up]) =>
      `<div style="display:flex;align-items:center;justify-content:space-between;padding:10px 0;border-bottom:1px solid #1f2937">
        <span style="font-size:13px;color:#d1d5db">${esc(name)}</span>
        ${badge(up ? 'running' : 'stopped', up ? 'green' : 'red')}
      </div>`
    ).join('');

    el.innerHTML = `<div style="padding:24px">
      ${pageHeader('Overview', 'CloudTwin runtime status', refreshBtn('loadOverview'))}
      <div style="margin-top:24px;display:grid;grid-template-columns:repeat(auto-fill,minmax(160px,1fr));gap:16px">
        ${statCard('Status',   d.status === 'ok' ? '✓ OK' : '⚠ Degraded', d.status === 'ok' ? '#4ade80' : '#fbbf24')}
        ${statCard('Storage',  d.storage_mode || 'unknown', '#60a5fa')}
        ${statCard('Services', `${ok} / ${entries.length}`, '#a78bfa')}
      </div>
      <div style="margin-top:24px">
        ${card('Service Status', serviceRows || emptyState('No services configured'))}
      </div>
    </div>`;
    reinitIcons();
  } catch (e) {
    el.innerHTML = `<div style="padding:24px">${errBanner(e.message)}</div>`;
  }
}
