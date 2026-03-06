// ─── Page: AWS SES ────────────────────────────────────────────────────────────
// Depends on: api, components, utils

async function loadSES() {
  const el = document.getElementById('page-aws-ses');
  try {
    const d = await api.aws.ses();
    const identities = d.identities || [];
    const messages   = d.messages   || [];

    const identityCols = [
      { header: 'Identity', render: r => `<span style="font-family:monospace;font-size:13px">${esc(r.identity)}</span>` },
      { header: 'Type',     render: r => badge(r.type, 'blue') },
      { header: 'Status',   render: r => badge(r.verified ? 'verified' : 'pending', r.verified ? 'green' : 'yellow') },
      { header: 'Created',  render: r => `<span style="color:#6b7280;font-size:12px">${fmtDate(r.created_at)}</span>` },
    ];

    const messageCols = [
      { header: 'From',    render: r => mono(r.source) },
      { header: 'To',      render: r => mono(r.destination) },
      { header: 'Subject', render: r => esc(r.subject) },
      { header: 'Sent',    render: r => `<span style="color:#6b7280;font-size:12px">${fmtDate(r.created_at)}</span>` },
    ];

    el.innerHTML = `<div style="padding:24px">
      ${pageHeader('SES', `${identities.length} identities · ${messages.length} messages`, refreshBtn('loadSES'))}
      <div style="margin-top:24px">
        ${card('Verified Identities', tableHtml(identityCols, identities))}
        ${card('Sent Messages',       tableHtml(messageCols, messages))}
      </div>
    </div>`;
    reinitIcons();
  } catch (e) {
    el.innerHTML = `<div style="padding:24px">${errBanner(e.message)}</div>`;
  }
}
