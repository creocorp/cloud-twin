// ─── GCP Cloud Functions page ─────────────────────────────────────────────────

async function loadCloudFunctions() {
  const el = document.getElementById('page-gcp-cloudfunctions');
  try {
    const d = await api.gcp.cloudFunctions();
    const functions = d.functions || [];

    const cols = [
      { header: 'Function',  render: r => `<span style="font-weight:500">${esc(r.name)}</span>` },
      { header: 'Runtime',   render: r => badge(r.runtime || '—', 'blue') },
      { header: 'Full Name', render: r => mono(r.full_name || '—') },
      { header: 'Created',   render: r => `<span style="color:#6b7280;font-size:12px">${fmtDate(r.created_at)}</span>` },
    ];

    el.innerHTML = `<div style="padding:24px">
      ${pageHeader('Cloud Functions', `${functions.length} function${functions.length !== 1 ? 's' : ''}`, refreshBtn('loadCloudFunctions'))}
      <div style="margin-top:24px">${card('Functions', tableHtml(cols, functions))}</div>
    </div>`;
    reinitIcons();
  } catch (e) {
    el.innerHTML = `<div style="padding:24px">${errBanner(e.message)}</div>`;
  }
}
