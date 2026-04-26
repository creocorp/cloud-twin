// ─── AWS Secrets Manager page ─────────────────────────────────────────────────

async function loadSecretsManager() {
  const el = document.getElementById('page-aws-secretsmanager');
  try {
    const d = await api.aws.secretsmanager();
    const secrets = d.secrets || [];

    const cols = [
      { header: 'Name',    render: r => `<span style="font-weight:500">${esc(r.name)}</span>` },
      { header: 'ARN',     render: r => mono(r.arn || '—') },
      { header: 'Created', render: r => `<span style="color:#6b7280;font-size:12px">${fmtDate(r.created_at)}</span>` },
    ];

    el.innerHTML = `<div style="padding:24px">
      ${pageHeader('Secrets Manager', `${secrets.length} secret${secrets.length !== 1 ? 's' : ''}`, refreshBtn('loadSecretsManager'))}
      <div style="margin-top:24px">${card('Secrets', tableHtml(cols, secrets))}</div>
    </div>`;
    reinitIcons();
  } catch (e) {
    el.innerHTML = `<div style="padding:24px">${errBanner(e.message)}</div>`;
  }
}
