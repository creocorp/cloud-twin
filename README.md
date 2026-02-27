# CloudTwin

CloudTwin is a lightweight, self-contained local runtime for multi-cloud services.
It runs AWS, Azure, and GCP service emulators inside a single process with no
external dependencies — no Docker, no MinIO, no real cloud accounts needed.

Point your existing SDK code at `http://localhost:4793` and everything just works.
State is stored in SQLite (or kept in-memory for CI/testing).

---

## Supported Services

| Provider | Service | SDK compatibility |
|---|---|---|
| AWS | SES (v1 + v2) | boto3 `ses` / `sesv2` |
| AWS | S3 | boto3 `s3` |
| AWS | SNS | boto3 `sns` |
| AWS | SQS | boto3 `sqs` |
| Azure | Blob Storage | `azure-storage-blob` |
| Azure | Service Bus | `azure-servicebus` |
| GCP | Cloud Storage | `google-cloud-storage` |
| GCP | Pub/Sub | `google-cloud-pubsub` |

---

## Installation

**From source (recommended for now):**

```bash
git clone https://github.com/your-org/cloud-twin.git
cd cloud-twin
pip install -e ".[dev]"
```

**Docker:**

```bash
docker build -f docker/Dockerfile -t cloudtwin .
docker run -p 4793:4793 cloudtwin
```

---

## Quick Start

Start the server:

```bash
python -m cloudtwin
# Listening on http://0.0.0.0:4793
```

Override your SDK's endpoint URL and supply any dummy credentials:

**AWS (boto3)**

```python
import boto3

s3 = boto3.client(
    "s3",
    endpoint_url="http://localhost:4793",
    aws_access_key_id="local",
    aws_secret_access_key="local",
    region_name="us-east-1",
)

s3.create_bucket(Bucket="my-bucket")
s3.put_object(Bucket="my-bucket", Key="hello.txt", Body=b"Hello, world!")
```

**Azure Blob Storage**

```python
from azure.storage.blob import BlobServiceClient

client = BlobServiceClient(
    account_url="http://localhost:4793/devstoreaccount1",
    credential="devstorekey",
)

client.create_container("my-container")
client.get_blob_client("my-container", "hello.txt").upload_blob(b"Hello, world!")
```

**GCP Cloud Storage**

```python
from google.cloud import storage
from google.auth.credentials import AnonymousCredentials

client = storage.Client(
    project="local-project",
    credentials=AnonymousCredentials(),
    client_options={"api_endpoint": "http://localhost:4793"},
)

bucket = client.create_bucket("my-bucket")
bucket.blob("hello.txt").upload_from_string(b"Hello, world!")
```

---

## Configuration

CloudTwin is configured via environment variables or a YAML file. Environment
variables take precedence.

| Variable | Default | Description |
|---|---|---|
| `CLOUDTWIN_HOST` | `0.0.0.0` | Bind address |
| `CLOUDTWIN_PORT` | `4793` | API port |
| `CLOUDTWIN_STORAGE_MODE` | `sqlite` | `sqlite` or `memory` |
| `CLOUDTWIN_DB_PATH` | `/data/cloudtwin.db` | SQLite database path (sqlite mode only) |
| `CLOUDTWIN_CONFIG_PATH` | `/config/cloudtwin.yml` | Path to optional YAML config file |

To use a config file, set `CLOUDTWIN_CONFIG_PATH` or place a file at the default
path. Any key not set falls back to the built-in default.

**Example `cloudtwin.yml`:**

```yaml
host: 0.0.0.0
port: 4793
storage:
  mode: sqlite
  db_path: /data/cloudtwin.db
```

---

## Storage Modes

| Mode | How to enable | Notes |
|---|---|---|
| `sqlite` (default) | `CLOUDTWIN_STORAGE_MODE=sqlite` | State persists across restarts |
| `memory` | `CLOUDTWIN_STORAGE_MODE=memory` | No file I/O — ideal for CI and ephemeral test runs |

---

## Contributing

See [docs/developer-guide.md](docs/developer-guide.md) for architecture,
design patterns, and instructions for adding new services or providers.
