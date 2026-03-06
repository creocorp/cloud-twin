// ─── Page: Azure Blob Storage ────────────────────────────────────────────────
// Depends on: api, components, utils

async function loadAzureBlob() {
  const el = document.getElementById('page-azure-blob');
  try {
    const d = await api.azure.blob();
    const containers = d.containers || [];

    const cols = [
      { header: 'Container', render: r => `<span style="font-family:monospace;font-size:13px;color:#60a5fa">${esc(r.name)}</span>` },
      { header: 'Blobs',     render: r => badge(String(r.blob_count || 0), 'blue') },
      { header: 'Created',   render: r => `<span style="color:#6b7280;font-size:12px">${fmtDate(r.created_at)}</span>` },
    ];

    el.innerHTML = `<div style="padding:24px">
      ${pageHeader('Azure Blob Storage', `${containers.length} container${containers.length !== 1 ? 's' : ''}`, refreshBtn('loadAzureBlob'))}
      <div style="margin-top:24px">${card('Containers', tableHtml(cols, containers))}</div>
    </div>`;
    reinitIcons();
  } catch (e) {
    el.innerHTML = `<div style="padding:24px">${errBanner(e.message)}</div>`;
  }
}
