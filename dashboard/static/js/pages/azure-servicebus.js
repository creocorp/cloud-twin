// ─── Page: Azure Service Bus ──────────────────────────────────────────────────
// Depends on: api, components, utils

async function loadAzureSB() {
  const el = document.getElementById('page-azure-servicebus');
  try {
    const d = await api.azure.serviceBus();
    const queues = d.queues || [];
    const topics = d.topics || [];

    const queueCols = [
      { header: 'Queue',    render: r => `<span style="font-weight:500">${esc(r.name)}</span>` },
      { header: 'Messages', render: r => badge(String(r.message_count || 0), (r.message_count || 0) > 0 ? 'yellow' : 'gray') },
      { header: 'Created',  render: r => `<span style="color:#6b7280;font-size:12px">${fmtDate(r.created_at)}</span>` },
    ];

    const topicCols = [
      { header: 'Topic',         render: r => `<span style="font-weight:500">${esc(r.name)}</span>` },
      { header: 'Subscriptions', render: r => badge(String(r.subscription_count || 0), 'blue') },
      { header: 'Created',       render: r => `<span style="color:#6b7280;font-size:12px">${fmtDate(r.created_at)}</span>` },
    ];

    el.innerHTML = `<div style="padding:24px">
      ${pageHeader('Azure Service Bus', `${queues.length} queues · ${topics.length} topics`, refreshBtn('loadAzureSB'))}
      <div style="margin-top:24px">
        ${card('Queues', tableHtml(queueCols, queues))}
        ${card('Topics', tableHtml(topicCols, topics))}
      </div>
    </div>`;
    reinitIcons();
  } catch (e) {
    el.innerHTML = `<div style="padding:24px">${errBanner(e.message)}</div>`;
  }
}
