// ─── Azure Queue Storage page ─────────────────────────────────────────────────

async function loadAzureQueue() {
  const el = document.getElementById('page-azure-queue');
  try {
    const d = await api.azure.queue();
    const queues = d.queues || [];

    const cols = [
      { header: 'Account', render: r => `<span style="font-weight:500">${esc(r.account)}</span>` },
      { header: 'Queue',   render: r => `<span style="font-weight:500">${esc(r.name)}</span>` },
      { header: 'Depth',   render: r => badge(String(r.message_count || 0), r.message_count > 0 ? 'yellow' : 'gray') },
      { header: 'Created', render: r => `<span style="color:#6b7280;font-size:12px">${fmtDate(r.created_at)}</span>` },
    ];

    el.innerHTML = `<div style="padding:24px">
      ${pageHeader('Queue Storage', `${queues.length} queue${queues.length !== 1 ? 's' : ''}`, refreshBtn('loadAzureQueue'))}
      <div style="margin-top:24px">${card('Queues', tableHtml(cols, queues))}</div>
    </div>`;
    reinitIcons();
  } catch (e) {
    el.innerHTML = `<div style="padding:24px">${errBanner(e.message)}</div>`;
  }
}
