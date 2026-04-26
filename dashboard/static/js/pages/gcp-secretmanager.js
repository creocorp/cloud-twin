// ─── GCP Secret Manager page ──────────────────────────────────────────────────

async function loadGcpSecretManager() {
  const el = document.getElementById('page-gcp-secretmanager');
  try {
    const d = await api.gcp.secretManager();
    const secrets = d.secrets || [];

    const cols = [
      { header: 'Name',      render: r => `<span style="font-weight:500">${esc(r.name)}</span>` },
      { header: 'Full Name', render: r => mono(r.full_name || '—') },
      { header: 'Created',   render: r => `<span style="color:#6b7280;font-size:12px">${fmtDate(r.created_at)}</span>` },
    ];

    el.innerHTML = `<div style="padding:24px">
      ${pageHeader('Secret Manager', `${secrets.length} secret${secrets.length !== 1 ? 's' : ''}`, refreshBtn('loadGcpSecretManager'))}
      <div style="margin-top:24px">${card('Secrets', tableHtml(cols, secrets))}</div>
    </div>`;
    reinitIcons();
  } catch (e) {
    el.innerHTML = `<div style="padding:24px">${errBanner(e.message)}</div>`;
  }
}
