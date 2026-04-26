// ─── Azure Key Vault page ─────────────────────────────────────────────────────

async function loadAzureKeyVault() {
  const el = document.getElementById('page-azure-keyvault');
  try {
    const d = await api.azure.keyVault();
    const secrets = d.secrets || [];

    const cols = [
      { header: 'Vault',   render: r => `<span style="font-weight:500">${esc(r.vault)}</span>` },
      { header: 'Name',    render: r => `<span style="font-weight:500">${esc(r.name)}</span>` },
      { header: 'Version', render: r => mono(r.version || '—') },
      { header: 'Created', render: r => `<span style="color:#6b7280;font-size:12px">${fmtDate(r.created_at)}</span>` },
    ];

    el.innerHTML = `<div style="padding:24px">
      ${pageHeader('Key Vault', `${secrets.length} secret${secrets.length !== 1 ? 's' : ''}`, refreshBtn('loadAzureKeyVault'))}
      <div style="margin-top:24px">${card('Secrets', tableHtml(cols, secrets))}</div>
    </div>`;
    reinitIcons();
  } catch (e) {
    el.innerHTML = `<div style="padding:24px">${errBanner(e.message)}</div>`;
  }
}
