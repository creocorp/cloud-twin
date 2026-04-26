// ─── Azure Event Grid page ────────────────────────────────────────────────────

async function loadAzureEventGrid() {
  const el = document.getElementById('page-azure-eventgrid');
  try {
    const d = await api.azure.eventGrid();
    const topics = d.topics || [];

    const cols = [
      { header: 'Topic',    render: r => `<span style="font-weight:500">${esc(r.name)}</span>` },
      { header: 'Endpoint', render: r => mono(r.endpoint || '—') },
      { header: 'Events',   render: r => badge(String(r.event_count || 0), r.event_count > 0 ? 'yellow' : 'gray') },
      { header: 'Created',  render: r => `<span style="color:#6b7280;font-size:12px">${fmtDate(r.created_at)}</span>` },
    ];

    el.innerHTML = `<div style="padding:24px">
      ${pageHeader('Event Grid', `${topics.length} topic${topics.length !== 1 ? 's' : ''}`, refreshBtn('loadAzureEventGrid'))}
      <div style="margin-top:24px">${card('Topics', tableHtml(cols, topics))}</div>
    </div>`;
    reinitIcons();
  } catch (e) {
    el.innerHTML = `<div style="padding:24px">${errBanner(e.message)}</div>`;
  }
}
