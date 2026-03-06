// ─── Page: AWS S3 ─────────────────────────────────────────────────────────────
// Depends on: api, components, utils

async function loadS3() {
  const el = document.getElementById('page-aws-s3');
  try {
    const d = await api.aws.s3();
    const buckets = d.buckets || [];

    const cols = [
      { header: 'Bucket',  render: r => `<span style="font-family:monospace;font-size:13px;color:#60a5fa">${esc(r.name)}</span>` },
      { header: 'Created', render: r => `<span style="color:#6b7280;font-size:12px">${fmtDate(r.created_at)}</span>` },
    ];

    el.innerHTML = `<div style="padding:24px">
      ${pageHeader('S3', `${buckets.length} bucket${buckets.length !== 1 ? 's' : ''}`, refreshBtn('loadS3'))}
      <div style="margin-top:24px">${card('Buckets', tableHtml(cols, buckets))}</div>
    </div>`;
    reinitIcons();
  } catch (e) {
    el.innerHTML = `<div style="padding:24px">${errBanner(e.message)}</div>`;
  }
}
