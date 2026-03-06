// ─── Page: GCP Pub/Sub ────────────────────────────────────────────────────────
// Depends on: api, components, utils

async function loadPubSub() {
  const el = document.getElementById('page-gcp-pubsub');
  try {
    const d = await api.gcp.pubsub();
    const topics = d.topics        || [];
    const subs   = d.subscriptions || [];

    const topicCols = [
      { header: 'Topic',         render: r => `<span style="font-family:monospace;font-size:13px;color:#38bdf8">${esc(r.name)}</span>` },
      { header: 'Subscriptions', render: r => badge(String(r.subscription_count || 0), 'blue') },
      { header: 'Created',       render: r => `<span style="color:#6b7280;font-size:12px">${fmtDate(r.created_at)}</span>` },
    ];

    const subCols = [
      { header: 'Subscription', render: r => `<span style="font-family:monospace;font-size:13px;color:#38bdf8">${esc(r.name)}</span>` },
      { header: 'Topic',        render: r => mono(r.topic) },
      { header: 'Messages',     render: r => badge(String(r.message_count || 0), (r.message_count || 0) > 0 ? 'yellow' : 'gray') },
      { header: 'Created',      render: r => `<span style="color:#6b7280;font-size:12px">${fmtDate(r.created_at)}</span>` },
    ];

    el.innerHTML = `<div style="padding:24px">
      ${pageHeader('GCP Pub/Sub', `${topics.length} topics · ${subs.length} subscriptions`, refreshBtn('loadPubSub'))}
      <div style="margin-top:24px">
        ${card('Topics',        tableHtml(topicCols, topics))}
        ${card('Subscriptions', tableHtml(subCols, subs))}
      </div>
    </div>`;
    reinitIcons();
  } catch (e) {
    el.innerHTML = `<div style="padding:24px">${errBanner(e.message)}</div>`;
  }
}
