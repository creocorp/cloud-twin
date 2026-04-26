// ─── Page: AWS Bedrock ────────────────────────────────────────────────────────
// Depends on: api, components, utils
//
// Renders the foundation-models table plus an interactive Chat Playground that
// invokes the live `/model/{id}/invoke` and `/model/{id}/invoke-with-response-stream`
// endpoints exposed by the Bedrock provider. The same UI works against both the
// Python backend and CloudTwin Lite (Rust) since both serve identical routes.

// In-memory chat history per page load. Keyed by model ID so switching models
// preserves each thread for the session.
const _bedrockChat = {
  history: {},   // { [modelId]: [{role, text, meta}] }
  current: null, // currently selected model ID
  busy: false,
  cols: null,    // columns config retained for live table re-renders
  lastModels: [],// last known models list (mutated in place on live refresh)
};

async function loadBedrock() {
  const el = document.getElementById('page-aws-bedrock');
  el.innerHTML = `<div style="padding:24px;color:#6b7280;font-size:13px">Loading…</div>`;
  try {
    const d = await api.aws.bedrock();
    const models = d.models || [];

    const totalRequests = models.reduce((s, m) => s + (m.request_count || 0), 0);

    if (!_bedrockChat.current && models.length > 0) {
      _bedrockChat.current = models[0].model_id;
    }

    const cols = [
      { header: 'Model ID',    render: r => mono(r.model_id) },
      { header: 'Name',        render: r => `<span style="color:#d1d5db;font-size:13px">${esc(r.model_name)}</span>` },
      { header: 'Provider',    render: r => badge(r.provider, 'blue') },
      { header: 'Invocations', render: r => `<span data-bedrock-count="${esc(r.model_id)}">${badge(String(r.request_count || 0), (r.request_count || 0) > 0 ? 'yellow' : 'gray')}</span>` },
    ];
    // Stash columns so we can re-render just the table after live updates.
    _bedrockChat.cols = cols;

    el.innerHTML = `<div style="padding:24px">
      ${pageHeader('Bedrock', `${models.length} models · ${totalRequests} invocations`, refreshBtn('loadBedrock'))}
      <div style="margin-top:24px">${card('Chat Playground', _bedrockChatHtml(models))}</div>
      <div id="bedrock-models-card">${card('Foundation Models', _bedrockModelsTableHtml(models))}</div>
    </div>`;
    _bedrockChat.lastModels = models;
    _bedrockRenderHistory();
    reinitIcons();
  } catch (e) {
    el.innerHTML = `<div style="padding:24px">${errBanner(e.message)}</div>`;
  }
}

// ── Chat panel HTML ─────────────────────────────────────────────────────────

function _bedrockChatHtml(models) {
  if (!models.length) {
    return `<div style="padding:32px;color:#6b7280;font-size:13px;text-align:center">
      No models configured. Add entries under <code>bedrock.models</code> in cloudtwin.yml to chat with them.
    </div>`;
  }

  const options = models.map(m => {
    const sel = m.model_id === _bedrockChat.current ? ' selected' : '';
    return `<option value="${esc(m.model_id)}"${sel}>${esc(m.model_id)}</option>`;
  }).join('');

  return `<div style="padding:16px 18px;display:flex;flex-direction:column;gap:12px">
    <div style="display:flex;align-items:center;gap:8px;flex-wrap:wrap">
      <label style="font-size:12px;color:#9ca3af;font-weight:500">Model</label>
      <select id="bedrock-chat-model" onchange="_bedrockOnModelChange(this.value)"
        style="background:#0b1220;border:1px solid #374151;color:#d1d5db;padding:6px 10px;border-radius:6px;font-size:13px;font-family:ui-monospace,monospace;min-width:280px">
        ${options}
      </select>
      <label style="font-size:12px;color:#9ca3af;display:inline-flex;align-items:center;gap:6px;margin-left:8px">
        <input type="checkbox" id="bedrock-chat-stream" style="accent-color:#60a5fa">
        stream
      </label>
      <div style="flex:1"></div>
      <button onclick="_bedrockClearChat()"
        style="display:inline-flex;align-items:center;gap:6px;padding:6px 12px;border-radius:6px;font-size:12px;background:#1f2937;border:1px solid #374151;color:#9ca3af;cursor:pointer">
        <i data-lucide="trash-2" style="width:13px;height:13px"></i> Clear
      </button>
    </div>

    <div id="bedrock-chat-output"
      style="height:280px;overflow-y:auto;background:#0b1220;border:1px solid #1f2937;border-radius:6px;padding:12px;font-size:13px;display:flex;flex-direction:column;gap:10px">
    </div>

    <form onsubmit="event.preventDefault();_bedrockSend()" style="display:flex;gap:8px;align-items:flex-end">
      <textarea id="bedrock-chat-input" rows="2" placeholder="Ask the model anything…"
        style="flex:1;background:#0b1220;border:1px solid #374151;color:#e5e7eb;padding:8px 10px;border-radius:6px;font-size:13px;font-family:inherit;resize:vertical"
        onkeydown="if(event.key==='Enter'&&!event.shiftKey){event.preventDefault();_bedrockSend()}"></textarea>
      <button type="submit" id="bedrock-chat-send"
        style="display:inline-flex;align-items:center;gap:6px;padding:9px 16px;border-radius:6px;font-size:13px;background:#1d4ed8;border:1px solid #2563eb;color:#fff;cursor:pointer;font-weight:500">
        <i data-lucide="send" style="width:13px;height:13px"></i> Send
      </button>
    </form>
    <p style="margin:0;font-size:11px;color:#6b7280">
      Responses are deterministic synthetic output from the configured simulation — not a real foundation model. Enter sends; Shift+Enter for newline.
    </p>
  </div>`;
}

function _bedrockOnModelChange(modelId) {
  _bedrockChat.current = modelId;
  _bedrockRenderHistory();
}

function _bedrockClearChat() {
  if (_bedrockChat.current) {
    _bedrockChat.history[_bedrockChat.current] = [];
  }
  _bedrockRenderHistory();
}

function _bedrockRenderHistory() {
  const out = document.getElementById('bedrock-chat-output');
  if (!out) return;
  const msgs = _bedrockChat.history[_bedrockChat.current] || [];
  if (!msgs.length) {
    out.innerHTML = `<div style="color:#4b5563;font-size:12px;text-align:center;padding:24px">
      No messages yet. Send a prompt to invoke <code>${esc(_bedrockChat.current || '')}</code>.
    </div>`;
    return;
  }
  out.innerHTML = msgs.map(_bedrockMsgHtml).join('');
  out.scrollTop = out.scrollHeight;
}

function _bedrockMsgHtml(m) {
  const isUser = m.role === 'user';
  const isErr  = m.role === 'error';
  const bg     = isUser ? '#0c1a3d' : isErr ? '#450a0a' : '#111827';
  const border = isUser ? '#1e3a8a' : isErr ? '#7f1d1d' : '#1f2937';
  const color  = isUser ? '#bfdbfe' : isErr ? '#fca5a5' : '#e5e7eb';
  const label  = isUser ? 'you' : isErr ? 'error' : 'assistant';
  const align  = isUser ? 'flex-end' : 'flex-start';
  const meta   = m.meta ? `<span style="color:#6b7280;font-size:10px;margin-left:8px">${esc(m.meta)}</span>` : '';
  // Pre-wrap so synthetic newlines render
  return `<div style="display:flex;flex-direction:column;align-items:${align};max-width:100%">
    <div style="font-size:10px;color:#6b7280;text-transform:uppercase;letter-spacing:0.05em;margin-bottom:2px">${label}${meta}</div>
    <div style="background:${bg};border:1px solid ${border};color:${color};padding:8px 12px;border-radius:8px;max-width:88%;white-space:pre-wrap;word-break:break-word;font-size:13px;line-height:1.5">${esc(m.text)}</div>
  </div>`;
}

async function _bedrockSend() {
  if (_bedrockChat.busy) return;
  const input  = document.getElementById('bedrock-chat-input');
  const sendBtn= document.getElementById('bedrock-chat-send');
  const stream = document.getElementById('bedrock-chat-stream')?.checked;
  const modelId = _bedrockChat.current;
  if (!modelId || !input) return;
  const prompt = (input.value || '').trim();
  if (!prompt) return;

  _bedrockChat.history[modelId] = _bedrockChat.history[modelId] || [];
  _bedrockChat.history[modelId].push({ role: 'user', text: prompt });
  input.value = '';
  _bedrockChat.busy = true;
  if (sendBtn) { sendBtn.disabled = true; sendBtn.style.opacity = '0.6'; }
  _bedrockRenderHistory();

  // Show a placeholder assistant bubble while we wait.
  const placeholder = { role: 'assistant', text: '…', meta: stream ? 'streaming' : 'invoke' };
  _bedrockChat.history[modelId].push(placeholder);
  _bedrockRenderHistory();

  const t0 = performance.now();
  try {
    const path = stream
      ? `/model/${encodeURIComponent(modelId)}/invoke-with-response-stream`
      : `/model/${encodeURIComponent(modelId)}/invoke`;
    const resp = await fetch(path, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'Accept': 'application/json' },
      body: JSON.stringify({ prompt, messages: [{ role: 'user', content: prompt }] }),
    });

    if (!resp.ok) {
      const errText = await resp.text();
      let msg = errText;
      try { msg = JSON.parse(errText).message || errText; } catch (_) {}
      placeholder.role = 'error';
      placeholder.text = `${resp.status} ${resp.statusText}: ${msg}`;
      placeholder.meta = `${Math.round(performance.now() - t0)}ms`;
    } else if (stream) {
      const buf = new Uint8Array(await resp.arrayBuffer());
      placeholder.text = _bedrockDecodeEventStream(buf) || '(empty stream)';
      placeholder.meta = `stream · ${Math.round(performance.now() - t0)}ms`;
    } else {
      const ctype = resp.headers.get('content-type') || '';
      let display;
      if (ctype.includes('json')) {
        const j = await resp.json();
        display = (typeof j.content === 'string') ? j.content : JSON.stringify(j, null, 2);
      } else {
        display = await resp.text();
      }
      placeholder.text = display || '(empty response)';
      const reqCount = resp.headers.get('x-cloudtwin-request-count');
      const source   = resp.headers.get('x-cloudtwin-response-source');
      const parts = [];
      if (source)   parts.push(source);
      if (reqCount) parts.push(`#${reqCount}`);
      parts.push(`${Math.round(performance.now() - t0)}ms`);
      placeholder.meta = parts.join(' · ');
    }
  } catch (e) {
    placeholder.role = 'error';
    placeholder.text = e.message || String(e);
    placeholder.meta = `${Math.round(performance.now() - t0)}ms`;
  } finally {
    _bedrockChat.busy = false;
    if (sendBtn) { sendBtn.disabled = false; sendBtn.style.opacity = ''; }
    _bedrockRenderHistory();
    reinitIcons();
    // Live-refresh the invocation counts in the models table without redrawing
    // the chat panel (which would lose focus/scroll position).
    _bedrockRefreshCounts();
  }
}

// ── Live-refresh the model invocation counts ────────────────────────────────

function _bedrockModelsTableHtml(models) {
  return tableHtml(_bedrockChat.cols, models);
}

async function _bedrockRefreshCounts() {
  try {
    const d = await api.aws.bedrock();
    const models = d.models || [];
    _bedrockChat.lastModels = models;

    // Update the page subtitle counter.
    const totalRequests = models.reduce((s, m) => s + (m.request_count || 0), 0);
    const subtitle = document.querySelector('#page-aws-bedrock h1 + p');
    if (subtitle) subtitle.textContent = `${models.length} models · ${totalRequests} invocations`;

    // Patch each row's invocation badge in place.
    for (const m of models) {
      const cell = document.querySelector(`[data-bedrock-count="${CSS.escape(m.model_id)}"]`);
      if (cell) {
        const n = m.request_count || 0;
        cell.innerHTML = badge(String(n), n > 0 ? 'yellow' : 'gray');
      }
    }
  } catch (_) {
    // Silent — the user can still hit the Refresh button manually.
  }
}

// Decode an AWS EventStream binary body: we only need the text fragments from
// `content_block_delta` events. This is a loose decoder that scans for embedded
// base64-encoded JSON payloads — sufficient for the synthetic streams produced
// by both backends, and tolerant of small framing differences.
function _bedrockDecodeEventStream(buf) {
  const out = [];
  // Frames: [4 total_len][4 hdr_len][4 prelude_crc][hdrs][payload][4 msg_crc]
  const dv = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
  let off = 0;
  while (off + 12 <= buf.length) {
    const totalLen = dv.getUint32(off);
    const hdrLen   = dv.getUint32(off + 4);
    if (totalLen < 16 || off + totalLen > buf.length) break;
    const payloadStart = off + 12 + hdrLen;
    const payloadEnd   = off + totalLen - 4;
    if (payloadEnd > payloadStart) {
      const payload = new TextDecoder().decode(buf.subarray(payloadStart, payloadEnd));
      try {
        const obj = JSON.parse(payload);
        if (typeof obj.bytes === 'string') {
          const inner = atob(obj.bytes);
          try {
            const innerObj = JSON.parse(inner);
            if (innerObj?.delta?.text) out.push(innerObj.delta.text);
            else if (innerObj?.type === 'message_stop') { /* skip */ }
            else out.push(inner);
          } catch { out.push(inner); }
        } else if (typeof obj.message === 'string') {
          out.push(`[error] ${obj.message}`);
        }
      } catch { /* non-JSON payload; ignore */ }
    }
    off += totalLen;
  }
  return out.join('').trimEnd();
}
