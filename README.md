# CloudTwin

CloudTwin is a lightweight, self-contained local runtime for multi-cloud services.
It emulates AWS, Azure, and GCP service APIs inside a single process with no
external dependencies — no MinIO, no RabbitMQ, no real cloud accounts required.

State is persisted in SQLite by default, or kept entirely in-memory for CI and
ephemeral test sessions. CloudTwin is designed to be a drop-in local replacement
for cloud services during development.

**Port:** `4793` → Cloud API and dashboard UI (`/dashboard` when enabled)

![CloudTwin dashboard overview](docs/screenshots/overview.png)

---

## Supported Services

| Provider | Service |
|---|---|
| AWS | SES (v1 + v2) |
| AWS | S3 |
| AWS | SNS |
| AWS | SQS |
| AWS | Lambda |
| AWS | DynamoDB |
| AWS | Secrets Manager |
| AWS | Bedrock (simulation) |
| Azure | Blob Storage |
| Azure | Service Bus |
| Azure | Queue Storage |
| Azure | Event Grid |
| Azure | Key Vault |
| Azure | Functions |
| GCP | Cloud Storage |
| GCP | Pub/Sub |
| GCP | Firestore |
| GCP | Cloud Tasks |
| GCP | Secret Manager |
| GCP | Cloud Functions |

For the full list of supported operations per service, see
[docs/developer-guide.md](docs/developer-guide.md#supported-operations).

---

## Running with Docker

The easiest way to get started is with the pre-built image from Docker Hub:

```bash
docker pull creogroup/cloudtwin:latest
docker run -p 4793:4793 creogroup/cloudtwin
```

To persist data across restarts, mount a volume at `/data`:

```bash
docker run -p 4793:4793 -v cloudtwin-data:/data creogroup/cloudtwin
```

To use an optional config file, mount it at `/config/cloudtwin.yml`:

```bash
docker run -p 4793:4793 \
  -v cloudtwin-data:/data \
  -v $(pwd)/config/cloudtwin.yml:/config/cloudtwin.yml \
  creogroup/cloudtwin
```

---

## Running from Source

```bash
git clone https://github.com/creocorp/cloud-twin.git
cd cloud-twin
pip install -e ".[dev]"
python -m cloudtwin
# Listening on http://0.0.0.0:4793
```

---

## AWS Bedrock Simulation

CloudTwin includes a fully config-driven Bedrock simulation engine. It does
**not** call real models — it produces deterministic synthetic responses for
testing SDK integration, retry logic, streaming handling, and prompt routing.

All output is **deterministically seeded** from `(model_id, field_path, request_count)`.
The same call always produces the same result — critical for stable tests and
snapshot assertions.

### Endpoints

| Method | Path | SDK operation |
|---|---|---|
| `GET` | `/foundation-models` | `bedrock.list_foundation_models()` |
| `POST` | `/model/{modelId}/invoke` | `bedrock-runtime.invoke_model()` |
| `POST` | `/model/{modelId}/invoke-with-response-stream` | `bedrock-runtime.invoke_model_with_response_stream()` |

All three endpoints are served by **both** the Python backend and CloudTwin
Lite (Rust). Responses include introspection headers:

| Header | Value |
|---|---|
| `x-cloudtwin-request-count` | Per-model invocation counter (1-based) |
| `x-cloudtwin-response-source` | How the response was resolved: `text`, `schema`, `static`, `rule`, `sequence[N]`, `error_injection` |

### Dashboard chat playground

The dashboard's **AWS → Bedrock** page includes an interactive chat window for
sending prompts to any configured model. It calls the live `/model/{id}/invoke`
and `/model/{id}/invoke-with-response-stream` endpoints, decodes the EventStream
binary frames in the browser, and shows the resolution source and request count
— useful for quickly probing scenario rules and streaming behaviour without
writing SDK code.

### Quick example

```python
import boto3, json

client = boto3.client(
    "bedrock-runtime",
    endpoint_url="http://localhost:4793",
    aws_access_key_id="test",
    aws_secret_access_key="test",
    region_name="us-east-1",
)

resp = client.invoke_model(
    modelId="anthropic.claude-3-sonnet-20240229-v1:0",
    body=json.dumps({"prompt": "Summarise this document"}).encode(),
    contentType="application/json",
    accept="application/json",
)
result = json.loads(resp["body"].read())
print(result["content"])  # synthetic lorem-ipsum text
```

### Resolution pipeline

When `invoke_model` or `invoke_model_with_response_stream` is called, the
engine resolves a response in strict priority order — **first match wins**:

1. **Error injection** — if the model has `errors:` configured and
   `request_count % every == 0`, return a synthetic AWS error immediately.
2. **Prompt rules** — scan `rules:` top-to-bottom; if any `contains:` substring
   matches the prompt (case-insensitive), return that rule's `response:` or
   `error:`.
3. **Sequence / cycle** — if the model has a `sequence:` block, select the
   response entry by request count (`sequence` clamps to last; `cycle` wraps).
4. **Fallback mode** — resolve using the model's `mode:` (`text`, `schema`, or
   `static`). If no mode is set, fall back to `defaults.mode` (default: `text`).

Unknown model IDs (not listed in `models:`) resolve using the global `defaults:`
— so the engine always returns a valid response, even for model IDs you haven't
configured.

### YAML configuration reference

The `bedrock:` section in `cloudtwin.yml` has two top-level keys:

```yaml
bedrock:
  defaults:             # fallback behaviour for unconfigured models
    mode: text          # text | schema | static
    latency:            # optional — applies to all models unless overridden
      min_ms: 50
      max_ms: 120

  models:
    <model_id>:         # every key appears in ListFoundationModels
      name: ...         # optional — derived from model_id if omitted
      provider: ...     # optional — derived from model_id prefix if omitted
      mode: text        # text | schema | static
      # ... per-feature config (see below)
```

---

#### Mode: `text` — synthetic lorem-ipsum

Returns a deterministic string of lorem-ipsum words seeded from the model ID
and request count.

```yaml
my-model.text:
  mode: text
  text:
    min_words: 10       # minimum words in response (default: 5)
    max_words: 30       # maximum words in response (default: 15)
```

Response body: `{ "content": "lorem ipsum dolor ...", "stop_reason": "end_turn", "model": "my-model.text" }`

---

#### Mode: `schema` — generated JSON matching a schema

Returns a JSON object with fake data generated recursively from a simplified
JSON Schema definition. Supported types: `object`, `array`, `string`, `number`,
`integer`, `boolean`. Arrays generate 1–3 items. `enum` selects cyclically.

```yaml
my-model.schema:
  mode: schema
  schema:
    type: object
    properties:
      summary:    { type: string }                     # → lorem-ipsum 2-word string
      confidence: { type: number }                     # → deterministic float (0.42, 0.73, …)
      tags:       { type: array, items: { type: string } }
      status:     { type: string, enum: [draft, published, archived] }  # cycles through values
      count:      { type: integer }                    # → deterministic int (1, 7, 42, …)
      active:     { type: boolean }                    # → alternates true/false
```

---

#### Mode: `static` — fixed payload

Returns the exact JSON object you configure, every time.

```yaml
my-model.static:
  mode: static
  static:
    result: fixed
    value: 42
```

---

#### Sequences and cycles

Define an ordered list of responses. In `sequence` mode the engine clamps to
the last entry once exhausted; in `cycle` mode it wraps around forever.

Each entry in `responses:` can be a `static:` payload, a `text:` config, or an
`error:`.

```yaml
# Sequence: first → second → second → second …
my-model.sequence:
  sequence:
    mode: sequence        # "sequence" (clamp) or "cycle" (wrap)
    responses:
      - static: { answer: first }
      - static: { answer: second }

# Cycle: a → b → a → b → a …
my-model.cycle:
  sequence:
    mode: cycle
    responses:
      - static: { answer: a }
      - static: { answer: b }

# Mixed entries: static, then text, then an error
my-model.mixed-seq:
  sequence:
    mode: sequence
    responses:
      - static: { step: "initial" }
      - text: { min_words: 3, max_words: 8 }
      - error: { type: ServiceUnavailableException, message: "Gone away" }
```

---

#### Prompt rules (`contains`)

Match on prompt content and return a specific response or error. Rules are
evaluated top-to-bottom; **first match wins**. If no rule matches, the engine
falls through to sequence/fallback.

```yaml
my-model.rules:
  mode: text                # fallback if no rule matches
  rules:
    - contains: sentiment
      response:
        static: { sentiment: positive, score: 0.9 }

    - contains: translate
      response:
        text: { min_words: 5, max_words: 10 }

    - contains: fail
      error:
        type: ValidationException
        message: "Prompt rejected by rule"
```

---

#### Error injection (`every: N`)

Fire a synthetic AWS error on every Nth invocation. Multiple entries are
supported — the first matching entry wins.

```yaml
my-model.flaky:
  mode: text
  errors:
    - every: 3
      type: ThrottlingException
      message: "Every 3rd request fails"
    - every: 10
      type: ServiceUnavailableException
      message: "Every 10th request is a 503"
```

---

#### Latency simulation

Add artificial delay before the response is sent. Configured per-model or
globally under `defaults:`. Per-model config overrides the global default.

```yaml
bedrock:
  defaults:
    latency:              # applies to all models unless overridden
      min_ms: 50
      max_ms: 200

  models:
    my-model.fast:
      mode: text
      latency:            # overrides the global default for this model
        min_ms: 0
        max_ms: 10
```

---

#### Streaming (`InvokeModelWithResponseStream`)

Control how the response is chunked into the AWS EventStream binary frames.
All chunk modes are supported by both Python and Rust backends.

```yaml
my-model.stream:
  mode: text
  streaming:
    enabled: true
    chunk_mode: word         # "word" | "char" | "fixed"
    fixed_chunk_size: 10     # bytes per chunk (only used when chunk_mode=fixed)
    first_chunk_delay_ms: 0  # delay before first chunk
    chunk_delay_ms: 20       # delay between subsequent chunks
    fail_after_chunks: 5     # inject a stream error after this many chunks (optional)
```

| `chunk_mode` | Behaviour |
|---|---|
| `word` (default) | Split on whitespace; each word is one event |
| `char` | Every character is a separate event |
| `fixed` | Fixed byte-width chunks of `fixed_chunk_size` |

Setting `fail_after_chunks` is useful for testing client-side stream error
recovery — the engine sends N valid chunks followed by a
`modelStreamErrorException` event.

---

#### Combining features

All features compose freely on a single model. The resolution pipeline applies
them in priority order, so you can have error injection + prompt rules +
sequence + streaming + latency all on the same model ID:

```yaml
my-model.kitchen-sink:
  mode: text
  text: { min_words: 10, max_words: 20 }
  latency: { min_ms: 50, max_ms: 150 }
  streaming:
    enabled: true
    chunk_mode: word
    chunk_delay_ms: 10
  errors:
    - every: 5
      type: ThrottlingException
      message: "Rate limited"
  rules:
    - contains: classify
      response: { static: { label: spam, confidence: 0.97 } }
  sequence:
    mode: cycle
    responses:
      - static: { answer: alpha }
      - static: { answer: beta }
```

On this model:
- Every 5th call → `ThrottlingException` (regardless of prompt)
- Prompt containing "classify" → `{ label: spam, confidence: 0.97 }`
- Otherwise → cycles between `alpha` and `beta`
- The `text:` fallback is only reached if `sequence:` is removed
- All successful responses are streamed word-by-word with 10ms inter-chunk delay
- A random 50–150ms latency is applied before any response

See [docs/developer-guide.md](docs/developer-guide.md#aws-bedrock-simulation) for additional architecture notes.

---

## Configuration

CloudTwin is configured via environment variables or a YAML file at
`CLOUDTWIN_CONFIG_PATH` (default `/config/cloudtwin.yml`). Environment
variables take precedence.

| Variable | Default | Description |
|---|---|---|
| `CLOUDTWIN_HOST` | `0.0.0.0` | Bind address |
| `CLOUDTWIN_API_PORT` | `4793` | API port used for both cloud endpoints and the dashboard UI |
| `CLOUDTWIN_STORAGE_MODE` | `sqlite` | `sqlite` or `memory` |
| `CLOUDTWIN_STORAGE_PATH` | `./data/cloudtwin.db` | SQLite database path |
| `CLOUDTWIN_CONFIG_PATH` | `/config/cloudtwin.yml` | Path to YAML config file |
| `CLOUDTWIN_DASHBOARD_ENABLED` | `true` | Mount the dashboard UI at `/dashboard` |

**Example `cloudtwin.yml`:**

```yaml
cloudtwin:
  api_port: 4793
  storage:
    mode: sqlite
    path: /data/cloudtwin.db
  providers:
    aws:
      services: ["ses", "s3", "sns", "sqs", "bedrock"]
  dashboard:
    enabled: true
  bedrock:
    defaults:
      mode: text
    models:
      anthropic.claude-3-sonnet-20240229-v1:0:
        name: Claude 3 Sonnet
        provider: anthropic
```

---

## Storage Modes

| Mode | How to enable | Notes |
|---|---|---|
| `sqlite` (default) | `CLOUDTWIN_STORAGE_MODE=sqlite` | State persists across restarts |
| `memory` | `CLOUDTWIN_STORAGE_MODE=memory` | No file I/O — ideal for CI and ephemeral test runs |

---

## Dashboard

The dashboard is served by the same FastAPI process and API port as CloudTwin.
When enabled, open `http://localhost:4793/dashboard`.

The UI route and backing endpoints are:

- UI entry: `/dashboard`
- Static assets: `/dashboard/static/...`
- JSON API: `/api/dashboard/*`

Enable or disable the UI mount via environment variable or config:

```bash
CLOUDTWIN_DASHBOARD_ENABLED=true python -m cloudtwin
```

Or in `cloudtwin.yml`:

```yaml
dashboard:
  enabled: true
```

The dashboard provides:

- **Overview** — live service health and recent event counts
- **Per-service pages** — browse and inspect resources for each service
  (SES identities and messages, S3 buckets and objects, SNS topics, SQS queues,
   Bedrock foundation models with an interactive **chat playground**,
   Azure Blob containers, Azure Service Bus queues/topics, GCP Cloud Storage
   buckets, GCP Pub/Sub topics and subscriptions)
- **Event log** — filterable stream of all actions emitted by the telemetry engine

| Overview | Bedrock Chat Playground |
|---|---|
| ![Overview](docs/screenshots/overview.png) | ![Bedrock chat](docs/screenshots/bedrock-chat.png) |

| SES — identities & sent messages | SNS — topics & subscriptions |
|---|---|
| ![SES](docs/screenshots/ses.png) | ![SNS](docs/screenshots/sns.png) |

The dashboard auto-polls the `/api/dashboard/*` endpoints. The backend router
for those endpoints is part of the main app; enabling the dashboard controls
whether the browser UI is mounted at `/dashboard`.

---

## Testing

```bash
# All integration tests (in-memory, no external services required)
make test-integration

# Directly
python -m pytest tests/integration/ -q
```

### Cross-implementation parity

The Python integration suite can be run against the Rust binary to verify
feature parity end-to-end. Set `CLOUDTWIN_TEST_URL` to skip spawning the
in-process Python app and point boto3/Azure/GCP clients at a running
backend instead:

```bash
make rust-build           # produce ./rust/cloudtwin-lite/target/release/cloudtwin-lite
make rust-test-parity     # boots the Rust binary on :47930 and runs the Bedrock suite against it
```

The Bedrock suite passes 100% against both backends. Other AWS services
(SES, S3, SNS, SQS, DynamoDB, Secrets Manager) share the same root URL
layout and are largely parity-tested, with a small number of pre-existing
edge-case gaps in CloudTwin Lite. Azure and GCP are mounted at different
paths (`/azure/`, `/gcp/`) in the Rust binary versus root in Python, so
their SDK fixtures are not directly portable across backends.

---

## CloudTwin Lite (Rust)

A single statically-linked binary with no Python runtime dependency, suitable
for embedding in CI pipelines or resource-constrained environments.
Source is under [`rust/cloudtwin-lite/`](rust/cloudtwin-lite/).

**Implemented services:** S3, SES (v1 + v2), SNS, SQS, DynamoDB, Secrets Manager,
**Bedrock (simulation)**, Azure Blob Storage, Azure Service Bus, GCP Cloud Storage,
GCP Pub/Sub.

**Run with Docker:**

```bash
docker pull creogroup/cloudtwin-lite:latest
docker run -p 4793:4793 creogroup/cloudtwin-lite
```

**Build from source:**

```bash
cd rust/cloudtwin-lite
cargo build --release
./target/release/cloudtwin-lite
# Listening on http://0.0.0.0:4793
```

Or via Make from the repo root:

```bash
make rust-build        # release build
make rust-run          # build + run
make rust-check        # cargo check only
```

**Configuration:**

Both CloudTwin Lite and the Python backend read the same `cloudtwin.yml` file
(path controlled by `CLOUDTWIN_CONFIG_PATH`, defaulting to `/config/cloudtwin.yml`).
Environment variables take precedence over YAML values where both apply.

| Variable | Default | Description |
|---|---|---|
| `CLOUDTWIN_CONFIG_PATH` | `/config/cloudtwin.yml` | Path to YAML config file (shared with Python backend) |
| `CLOUDTWIN_PORT` | `4793` | Port to listen on (overrides `api_port` in YAML) |
| `CLOUDTWIN_DB_PATH` | `/data/cloudtwin-lite.db` | SQLite database path (`:memory:` for in-memory) |

**Routing:**

| Provider | Base path |
|---|---|
| AWS (S3, SES, SNS, SQS, DynamoDB, Secrets Manager) | `/` |
| Azure (Blob Storage, Service Bus) | `/azure/` |
| GCP (Cloud Storage, Pub/Sub) | `/gcp/` |

See [docs/developer-guide.md](docs/developer-guide.md) for architecture and contribution notes.

---

## Contributing

See [docs/developer-guide.md](docs/developer-guide.md) for architecture,
design patterns, and instructions for adding new services or providers.

