// ─── GCP Cloud Tasks page ─────────────────────────────────────────────────────

async function loadCloudTasks() {
  const el = document.getElementById('page-gcp-cloudtasks');
  try {
    const d = await api.gcp.cloudTasks();
    const queues = d.queues || [];

    const cols = [
      { header: 'Queue',   render: r => `<span style="font-weight:500">${esc(r.name)}</span>` },
      { header: 'Tasks',   render: r => badge(String(r.task_count || 0), r.task_count > 0 ? 'yellow' : 'gray') },
      { header: 'Created', render: r => `<span style="color:#6b7280;font-size:12px">${fmtDate(r.created_at)}</span>` },
    ];

    el.innerHTML = `<div style="padding:24px">
      ${pageHeader('Cloud Tasks', `${queues.length} queue${queues.length !== 1 ? 's' : ''}`, refreshBtn('loadCloudTasks'))}
      <div style="margin-top:24px">${card('Queues', tableHtml(cols, queues))}</div>
    </div>`;
    reinitIcons();
  } catch (e) {
    el.innerHTML = `<div style="padding:24px">${errBanner(e.message)}</div>`;
  }
}
