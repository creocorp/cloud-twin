// ─── AWS DynamoDB page ────────────────────────────────────────────────────────

async function loadDynamoDB() {
  const el = document.getElementById('page-aws-dynamodb');
  try {
    const d = await api.aws.dynamodb();
    const tables = d.tables || [];

    const cols = [
      { header: 'Table',   render: r => `<span style="font-weight:500">${esc(r.name)}</span>` },
      { header: 'Status',  render: r => badge(r.status || 'ACTIVE', (r.status || 'ACTIVE') === 'ACTIVE' ? 'green' : 'yellow') },
      { header: 'Created', render: r => `<span style="color:#6b7280;font-size:12px">${fmtDate(r.created_at)}</span>` },
    ];

    el.innerHTML = `<div style="padding:24px">
      ${pageHeader('DynamoDB', `${tables.length} table${tables.length !== 1 ? 's' : ''}`, refreshBtn('loadDynamoDB'))}
      <div style="margin-top:24px">${card('Tables', tableHtml(cols, tables))}</div>
    </div>`;
    reinitIcons();
  } catch (e) {
    el.innerHTML = `<div style="padding:24px">${errBanner(e.message)}</div>`;
  }
}
