// ─── Azure Functions page ─────────────────────────────────────────────────────

async function loadAzureFunctions() {
  const el = document.getElementById('page-azure-functions');
  try {
    const d = await api.azure.functions();
    const functions = d.functions || [];

    const cols = [
      { header: 'App',      render: r => `<span style="font-weight:500">${esc(r.app)}</span>` },
      { header: 'Function', render: r => `<span style="font-weight:500">${esc(r.name)}</span>` },
      { header: 'Created',  render: r => `<span style="color:#6b7280;font-size:12px">${fmtDate(r.created_at)}</span>` },
    ];

    el.innerHTML = `<div style="padding:24px">
      ${pageHeader('Azure Functions', `${functions.length} function${functions.length !== 1 ? 's' : ''}`, refreshBtn('loadAzureFunctions'))}
      <div style="margin-top:24px">${card('Functions', tableHtml(cols, functions))}</div>
    </div>`;
    reinitIcons();
  } catch (e) {
    el.innerHTML = `<div style="padding:24px">${errBanner(e.message)}</div>`;
  }
}
