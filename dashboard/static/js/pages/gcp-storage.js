// ─── Page: GCP Cloud Storage ──────────────────────────────────────────────────
// Depends on: api, components, utils

async function loadGCSStorage() {
  const el = document.getElementById('page-gcp-storage');
  try {
    const d = await api.gcp.storage();
    const buckets = d.buckets || [];

    const cols = [
      { header: 'Bucket',   render: r => `<span style="font-family:monospace;font-size:13px;color:#38bdf8">${esc(r.name)}</span>` },
      { header: 'Location', render: r => badge(r.location || '—', 'gray') },
      { header: 'Objects',  render: r => badge(String(r.object_count || 0), 'blue') },
      { header: 'Created',  render: r => `<span style="color:#6b7280;font-size:12px">${fmtDate(r.created_at)}</span>` },
    ];

    el.innerHTML = `<div style="padding:24px">
      ${pageHeader('GCP Cloud Storage', `${buckets.length} bucket${buckets.length !== 1 ? 's' : ''}`, refreshBtn('loadGCSStorage'))}
      <div style="margin-top:24px">${card('Buckets', tableHtml(cols, buckets))}</div>
    </div>`;
    reinitIcons();
  } catch (e) {
    el.innerHTML = `<div style="padding:24px">${errBanner(e.message)}</div>`;
  }
}
