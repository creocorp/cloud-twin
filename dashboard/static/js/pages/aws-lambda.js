// ─── AWS Lambda page ──────────────────────────────────────────────────────────

async function loadLambda() {
  const el = document.getElementById('page-aws-lambda');
  try {
    const d = await api.aws.lambda();
    const functions = d.functions || [];

    const cols = [
      { header: 'Function', render: r => `<span style="font-weight:500">${esc(r.name)}</span>` },
      { header: 'Runtime',  render: r => badge(r.runtime || '—', 'blue') },
      { header: 'ARN',      render: r => mono(r.arn || '—') },
      { header: 'Created',  render: r => `<span style="color:#6b7280;font-size:12px">${fmtDate(r.created_at)}</span>` },
    ];

    el.innerHTML = `<div style="padding:24px">
      ${pageHeader('Lambda', `${functions.length} function${functions.length !== 1 ? 's' : ''}`, refreshBtn('loadLambda'))}
      <div style="margin-top:24px">${card('Functions', tableHtml(cols, functions))}</div>
    </div>`;
    reinitIcons();
  } catch (e) {
    el.innerHTML = `<div style="padding:24px">${errBanner(e.message)}</div>`;
  }
}
