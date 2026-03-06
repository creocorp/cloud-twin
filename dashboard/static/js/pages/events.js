// ─── Page: Event Log ─────────────────────────────────────────────────────────
// Depends on: api, components, utils

async function loadEvents() {
  const el = document.getElementById('page-events');
  try {
    const d = await api.events(100);
    const events = [...(d.events || [])].reverse();

    const providerColor = p => ({ aws: 'yellow', azure: 'blue', gcp: 'purple' }[p] || 'gray');

    const cols = [
      { header: '#',        render: r => `<span style="color:#4b5563">${esc(r.id)}</span>` },
      { header: 'Provider', render: r => badge(r.provider, providerColor(r.provider)) },
      { header: 'Service',  render: r => `<span style="font-weight:500">${esc(r.service)}</span>` },
      {
        header: 'Action',
        render: r => `<code style="font-family:monospace;font-size:12px;background:#0f172a;padding:2px 6px;border-radius:4px;color:#a78bfa">${esc(r.action)}</code>`,
      },
      { header: 'Time',     render: r => `<span style="color:#6b7280;font-size:12px">${fmtDate(r.created_at)}</span>` },
    ];

    el.innerHTML = `<div style="padding:24px">
      ${pageHeader('Event Log', 'Live telemetry — last 100 events', badge(String(events.length), 'blue'))}
      <div style="margin-top:24px">
        ${card('Events', tableHtml(cols, events))}
      </div>
    </div>`;
    reinitIcons();
  } catch (e) {
    el.innerHTML = `<div style="padding:24px">${errBanner(e.message)}</div>`;
  }
}
