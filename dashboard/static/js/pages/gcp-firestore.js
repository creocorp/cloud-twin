// ─── GCP Firestore page ───────────────────────────────────────────────────────

async function loadFirestore() {
  const el = document.getElementById('page-gcp-firestore');
  try {
    const d = await api.gcp.firestore();
    const collections = d.collections || [];

    const cols = [
      { header: 'Collection', render: r => `<span style="font-weight:500">${esc(r.name)}</span>` },
      { header: 'Documents',  render: r => badge(String(r.document_count || 0), r.document_count > 0 ? 'blue' : 'gray') },
    ];

    el.innerHTML = `<div style="padding:24px">
      ${pageHeader('Firestore', `${collections.length} collection${collections.length !== 1 ? 's' : ''}`, refreshBtn('loadFirestore'))}
      <div style="margin-top:24px">${card('Collections', tableHtml(cols, collections))}</div>
    </div>`;
    reinitIcons();
  } catch (e) {
    el.innerHTML = `<div style="padding:24px">${errBanner(e.message)}</div>`;
  }
}
