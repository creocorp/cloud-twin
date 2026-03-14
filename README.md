# CloudTwin

CloudTwin is a lightweight, self-contained local runtime for multi-cloud services.
It emulates AWS, Azure, and GCP service APIs inside a single process with no
external dependencies — no MinIO, no RabbitMQ, no real cloud accounts required.

State is persisted in SQLite by default, or kept entirely in-memory for CI and
ephemeral test sessions. CloudTwin is designed to be a drop-in local replacement
for cloud services during development.

**Port:** `4793` → Cloud API and dashboard UI (`/dashboard` when enabled)

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

### Supported features

- `text` — synthetic lorem-ipsum text generation
- `schema` — JSON object generated from a simplified JSON Schema definition
- `static` — fixed configured payload returned verbatim
- Sequences and cycles — deterministic multi-response progressions
- Prompt rules — `contains`-based rule matching for response selection
- Error injection — fire a configured error every N requests
- Latency simulation — configurable min/max response delay
- Streaming — `InvokeModelWithResponseStream` with word, char, or fixed-char chunking

### Endpoints

| Method | Path | SDK operation |
|---|---|---|
| `GET` | `/foundation-models` | `bedrock.list_foundation_models()` |
| `POST` | `/model/{modelId}/invoke` | `bedrock-runtime.invoke_model()` |
| `POST` | `/model/{modelId}/invoke-with-response-stream` | `bedrock-runtime.invoke_model_with_response_stream()` |

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

### YAML configuration

```yaml
bedrock:
  defaults:
    mode: text
    latency:
      min_ms: 50
      max_ms: 120

  models:
    anthropic.claude-3-sonnet-20240229-v1:0:
      mode: schema
      schema:
        type: object
        properties:
          summary:
            type: string
          confidence:
            type: number
          tags:
            type: array
            items:
              type: string
      streaming:
        enabled: true
        chunk_mode: word
        first_chunk_delay_ms: 150
        chunk_delay_ms: 20

    custom.error-model:
      mode: text
      errors:
        - every: 5
          type: ThrottlingException
          message: "Every 5th request is throttled"
```

See [docs/developer-guide.md](docs/developer-guide.md#aws-bedrock-simulation) for the full reference.

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
host: 0.0.0.0
port: 4793
storage:
  mode: sqlite
  db_path: /data/cloudtwin.db
dashboard:
  enabled: true
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
   Azure Blob containers, Azure Service Bus queues/topics, GCP Cloud Storage
   buckets, GCP Pub/Sub topics and subscriptions)
- **Event log** — filterable stream of all actions emitted by the telemetry engine

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

---

## CloudTwin Lite (Rust)

A single statically-linked binary with no Python runtime dependency, suitable
for embedding in CI pipelines or resource-constrained environments.
Source is under [`rust/cloudtwin-lite/`](rust/cloudtwin-lite/).

**Implemented services:** S3, SES (v1 + v2), SNS, SQS, DynamoDB, Secrets Manager,
Azure Blob Storage, Azure Service Bus, GCP Cloud Storage, GCP Pub/Sub.

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

**Configuration (environment variables):**

| Variable | Default | Description |
|---|---|---|
| `CLOUDTWIN_PORT` | `4793` | Port to listen on |
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

