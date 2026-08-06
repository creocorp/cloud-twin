// ─── Page: Mailbox (Mailpit-style email capture) ──────────────────────────────
// Depends on: api, components, utils
//
// Top-level feature (not tied to a cloud provider). Lists every email captured
// by the SMTP server or funneled in from AWS SES, and renders a detail view for
// a selected message.

async function loadMailbox() {
  const el = document.getElementById('page-mailbox');
  try {
    const d = await api.mailbox();
    const messages = d.messages || [];

    const cols = [
      { header: 'From',    render: r => mono(r.from || '—') },
      { header: 'To',      render: r => mono(r.to || '—') },
      {
        header: 'Subject',
        render: r => `<a href="javascript:void(0)" onclick="loadMailboxMessage('${esc(r.id)}')" style="color:#60a5fa;text-decoration:none">${esc(r.subject || '(no subject)')}</a>`,
      },
      { header: 'Source',  render: r => badge(r.source, r.source === 'ses' ? 'purple' : 'blue') },
      { header: 'Att',     render: r => (r.attachments ? badge(String(r.attachments), 'gray') : '') },
      { header: 'Size',    render: r => `<span style="color:#6b7280;font-size:12px">${fmtBytes(r.size)}</span>` },
      { header: 'Received', render: r => `<span style="color:#6b7280;font-size:12px">${fmtDate(r.created_at)}</span>` },
    ];

    el.innerHTML = `<div style="padding:24px">
      ${pageHeader('Mailbox', `${messages.length} captured message${messages.length === 1 ? '' : 's'} · SMTP listening on :${d.smtp_port || 1025}`, refreshBtn('loadMailbox'))}
      <div style="margin-top:24px">
        ${card('Inbox', tableHtml(cols, messages))}
        <div id="mailbox-detail"></div>
      </div>
    </div>`;
    reinitIcons();
  } catch (e) {
    el.innerHTML = `<div style="padding:24px">${errBanner(e.message)}</div>`;
  }
}

async function loadMailboxMessage(id) {
  const host = document.getElementById('mailbox-detail');
  if (!host) return;
  host.innerHTML = spinner();
  try {
    const m = await api.mailboxMessage(id);
    if (m.error) {
      host.innerHTML = errBanner('Message not found');
      return;
    }

    const meta = [
      ['From', m.from],
      ['To', (m.to || []).join(', ')],
      ['Cc', (m.cc || []).join(', ')],
      ['Subject', m.subject || '(no subject)'],
    ]
      .filter(([, v]) => v)
      .map(
        ([k, v]) => `<div style="display:flex;gap:8px;padding:3px 0">
          <span style="color:#6b7280;font-size:12px;width:64px;flex-shrink:0">${k}</span>
          <span style="font-size:13px;color:#d1d5db;word-break:break-word">${esc(v)}</span>
        </div>`
      )
      .join('');

    let bodyHtml;
    if (m.html_body) {
      // Render untrusted email HTML inside a sandboxed iframe (no scripts) so
      // captured markup can't execute or touch the dashboard. The srcdoc is set
      // via the DOM property below rather than an attribute to avoid escaping
      // pitfalls.
      bodyHtml = `<iframe id="mailbox-iframe" sandbox="" style="width:100%;height:420px;border:0;background:#fff;border-radius:6px"></iframe>`;
    } else if (m.text_body) {
      bodyHtml = `<pre style="white-space:pre-wrap;word-break:break-word;font-size:13px;color:#d1d5db;margin:0">${esc(m.text_body)}</pre>`;
    } else {
      bodyHtml = emptyState('No readable body');
    }

    const attachments = m.attachments || [];
    const attHtml = attachments.length
      ? `<div style="margin-top:16px">${card(
          'Attachments',
          tableHtml(
            [
              { header: 'Filename', render: a => mono(a.filename) },
              { header: 'Type', render: a => badge(a.content_type, 'gray') },
              { header: 'Size', render: a => `<span style="color:#6b7280;font-size:12px">${fmtBytes(a.size)}</span>` },
            ],
            attachments
          )
        )}</div>`
      : '';

    host.innerHTML = `<div style="margin-top:8px">
      ${card(
        `Message · ${badge(m.source, m.source === 'ses' ? 'purple' : 'blue')}`,
        `<div style="padding:16px">
           ${meta}
           <div style="margin-top:14px;border-top:1px solid #1f2937;padding-top:14px">${bodyHtml}</div>
         </div>`
      )}
      ${attHtml}
      <details style="margin-top:8px">
        <summary style="cursor:pointer;color:#6b7280;font-size:12px;padding:6px 0">View raw source</summary>
        <pre style="white-space:pre-wrap;word-break:break-word;font-size:12px;color:#9ca3af;background:#111827;border:1px solid #1f2937;border-radius:8px;padding:14px;margin:6px 0 0">${esc(m.raw || '')}</pre>
      </details>
    </div>`;
    reinitIcons();
    // Safely populate the sandboxed iframe (if present) via the DOM property.
    if (m.html_body) {
      const frame = document.getElementById('mailbox-iframe');
      if (frame) frame.srcdoc = m.html_body;
    }
    host.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
  } catch (e) {
    host.innerHTML = errBanner(e.message);
  }
}
