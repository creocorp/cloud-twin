// ─── Page: AWS SQS ────────────────────────────────────────────────────────────
// Depends on: api, components, utils

async function loadSQS() {
  const el = document.getElementById('page-aws-sqs');
  try {
    const d = await api.aws.sqs();
    const queues = d.queues || [];

    const cols = [
      { header: 'Queue',   render: r => `<span style="font-weight:500">${esc(r.name)}</span>` },
      { header: 'Depth',   render: r => badge(String(r.message_count || 0), (r.message_count || 0) > 0 ? 'yellow' : 'gray') },
      { header: 'URL',     render: r => mono(r.url) },
      { header: 'Created', render: r => `<span style="color:#6b7280;font-size:12px">${fmtDate(r.created_at)}</span>` },
    ];

    el.innerHTML = `<div style="padding:24px">
      ${pageHeader('SQS', `${queues.length} queue${queues.length !== 1 ? 's' : ''}`, refreshBtn('loadSQS'))}
      <div style="margin-top:24px">${card('Queues', tableHtml(cols, queues))}</div>
    </div>`;
    reinitIcons();
  } catch (e) {
    el.innerHTML = `<div style="padding:24px">${errBanner(e.message)}</div>`;
  }
}
