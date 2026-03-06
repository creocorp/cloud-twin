// ─── Page: AWS SNS ────────────────────────────────────────────────────────────
// Depends on: api, components, utils

async function loadSNS() {
  const el = document.getElementById('page-aws-sns');
  try {
    const d = await api.aws.sns();
    const topics = d.topics        || [];
    const subs   = d.subscriptions || [];

    const topicCols = [
      { header: 'Name',    render: r => `<span style="font-weight:500">${esc(r.name)}</span>` },
      { header: 'ARN',     render: r => mono(r.arn) },
      { header: 'Created', render: r => `<span style="color:#6b7280;font-size:12px">${fmtDate(r.created_at)}</span>` },
    ];

    const subCols = [
      { header: 'Protocol', render: r => badge(r.protocol, 'blue') },
      { header: 'Endpoint', render: r => mono(r.endpoint) },
      { header: 'Topic',    render: r => mono(r.topic_arn) },
    ];

    el.innerHTML = `<div style="padding:24px">
      ${pageHeader('SNS', `${topics.length} topics · ${subs.length} subscriptions`, refreshBtn('loadSNS'))}
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
