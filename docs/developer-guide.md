# CloudTwin — Developer Guide

This guide is for contributors and anyone wanting to understand, extend, or
modify the CloudTwin codebase.

---

## Contents

- [Architecture overview](#architecture-overview)
- [Directory layout](#directory-layout)
- [Core design principles](#core-design-principles)
- [Key patterns](#key-patterns)
  - [Service pattern](#service-pattern)
  - [Handler pattern](#handler-pattern)
  - [Repository pattern](#repository-pattern)
  - [Error handling](#error-handling)
- [Supported operations](#supported-operations)
- [Adding a new AWS service](#adding-a-new-aws-service)
- [Adding a new cloud provider](#adding-a-new-cloud-provider)
- [AWS protocol reference](#aws-protocol-reference)
- [Configuration internals](#configuration-internals)
- [Dashboard](#dashboard)
- [Running tests](#running-tests)
- [CloudTwin Lite (Rust)](#cloudtwin-lite-rust)

---

## Architecture Overview

CloudTwin is a FastAPI application. On startup it instantiates one _provider_
per cloud (`AwsProvider`, `AzureProvider`, `GcpProvider`). Each provider
discovers its services and calls their `register()` function, which mounts
HTTP routes and wires up dependencies.

All persistence is handled through _repository_ abstractions backed by SQLite
(`aiosqlite`). Services depend only on repository interfaces and never look at
which connection is active.

```
Request → FastAPI Router → Handler (protocol adapter)
                              ↓
                          Service (business logic)
                              ↓
                          Repository (abstract)
                              ↓
                       SQLiteRepository
                      (file or :memory:)
```

Telemetry events are fire-and-forget writes to an `events` table for future
dashboard consumption.

---

## Directory Layout

```
src/cloudtwin/
  app.py                        # FastAPI application factory (create_app)
  config.py                     # Dataclass config + YAML/env loader (load_config)
  __main__.py                   # Entry point: python -m cloudtwin

  core/
    errors.py                   # Shared exception hierarchy (CloudTwinError)
    xml.py                      # AWS XML response builders (SES v1, SNS)
    telemetry.py                # TelemetryEngine – fire-and-forget event sink
    routing.py                  # Thin APIRouter factory

  persistence/
    db.py                       # Database – aiosqlite connection + DDL bootstrap
                                #   (DDL is collected from each service's sqlite.py module)
    models/
      __init__.py               # Flat re-export of all models
      aws/
        ses.py                  # SesIdentity, SesMessage
        s3.py                   # S3Bucket, S3Object
        sns.py                  # SnsTopic, SnsSubscription, SnsMessage
        sqs.py                  # SqsQueue, SqsMessage
      azure/
        blob.py                 # AzureContainer, AzureBlob
        servicebus.py           # AsbQueue, AsbTopic, AsbSubscription, AsbMessage
      gcp/
        storage.py              # GcsBucket, GcsObject
        pubsub.py               # PubsubTopic, PubsubSubscription, PubsubMessage, PubsubAckable
      common/
        events.py               # Event
    repositories/
      __init__.py               # make_repositories() factory + all re-exports
      aws/
        ses/                    # repository.py · sqlite.py (DDL) · __init__.py
        s3/                     # repository.py · sqlite.py (DDL) · __init__.py
        sns/                    # repository.py · sqlite.py (DDL) · __init__.py
        sqs/                    # repository.py · sqlite.py (DDL) · __init__.py
      azure/
        blob/                   # repository.py · sqlite.py (DDL) · __init__.py
        servicebus/             # repository.py · sqlite.py (DDL) · __init__.py
      gcp/
        storage/                # repository.py · sqlite.py (DDL) · __init__.py
        pubsub/                 # repository.py · sqlite.py (DDL) · __init__.py
      common/
        events/                 # repository.py · sqlite.py (DDL) · __init__.py

  providers/
    aws/
      provider.py               # AwsProvider – discovers + registers services,
                                #   owns the single POST / endpoint
      protocols/
        query.py                # AWS Query dispatcher (SES v1, SNS – form-urlencoded)
        json_protocol.py        # AWS JSON dispatcher (SQS – X-Amz-Target header)
      ses/                      # SES service (v1 + v2)
      s3/                       # S3 service
      sns/                      # SNS service
      sqs/                      # SQS service
    azure/
      provider.py               # AzureProvider
      blob/                     # Azure Blob Storage service
      servicebus/               # Azure Service Bus service
    gcp/
      provider.py               # GcpProvider
      storage/                  # GCP Cloud Storage service
      pubsub/                   # GCP Pub/Sub service

tests/
  integration/
    providers/
      aws/                      # SES v1/v2, S3, SNS, SQS boto3 tests
      azure/                    # Azure Blob, Service Bus SDK tests
      gcp/                      # GCS, Pub/Sub SDK tests
```

---

## Core Design Principles

1. **Single runtime, no external services.** No MinIO, no RabbitMQ, no mail
   servers. Everything runs in-process.

2. **SQLite as source of truth.** All services store state in SQLite via
   `aiosqlite`. Both storage modes use the same `Sqlite*` repository
   implementations: `mode="sqlite"` persists to a file; `mode="memory"` opens
   an in-memory SQLite connection (`":memory:"`), which is fast, isolated, and
   requires no file I/O — ideal for CI and ephemeral testing.

3. **Services are pure Python.** `service.py` files contain only domain logic.
   They have no knowledge of HTTP, XML, or JSON. They depend only on repository
   interfaces and `TelemetryEngine`.

4. **Protocol adapter pattern.** HTTP and protocol concerns live in `handlers.py`
   (and `handlers_v2.py`). Handlers parse requests, call service methods, and
   format responses. They never contain business logic.

5. **Self-registering services.** Each service package exposes a single
   `register(app, config, repos, telemetry)` function via its `__init__.py`.
   `provider.py` discovers services by name and calls `register` — it contains
   no service-specific code.

6. **Repository abstraction.** Services depend on abstract repository base
   classes. One concrete implementation exists per entity: `SqliteXxxRepository`.
   It works transparently with both file-backed and `:memory:` SQLite connections.
   Services never check which storage mode is active.

---

## Key Patterns

### Service Pattern

```python
# service.py – no HTTP, no XML, no JSON
class FooService:
    def __init__(self, config, repo: FooRepository, telemetry: TelemetryEngine):
        self._config = config
        self._repo = repo
        self._telemetry = telemetry

    async def do_thing(self, param: str) -> str:
        # validate → mutate state via repo → emit telemetry
        result = await self._repo.save(...)
        await self._telemetry.emit("aws", "foo", "do_thing", {"param": param})
        return result
```

### Handler Pattern

```python
# handlers.py – HTTP only, delegates everything to service
@router.post("/foo")
async def create_foo(request: Request):
    body = await request.json()
    try:
        result = await service.do_thing(body["Field"])
    except CloudTwinError as exc:
        return JSONResponse({"message": exc.message}, status_code=exc.http_status)
    return JSONResponse({"Result": result})
```

For **AWS Query protocol** (form-urlencoded) handlers, register the action into
the `query_router` rather than a FastAPI router:

```python
async def handle_create_foo(form: dict) -> Response:
    ...

query_router.register("CreateFoo", handle_create_foo)
```

### Repository Pattern

Every new entity needs:

- A dataclass in `persistence/models/<provider>/<service>.py`
- An abstract class in `persistence/repositories/<provider>/<service>/repository.py`
  (inherits `ABC`)
- A `sqlite.py` with the SQLite implementation **and** a module-level `DDL` constant
- An `__init__.py` re-exporting all classes
- The `DDL` imported and concatenated in `persistence/db.py`
- An entry in `make_repositories()` in `persistence/repositories/__init__.py`

**Example abstract repository:**

```python
class FooRepository(ABC):
    @abstractmethod
    async def get(self, id: int) -> Optional[Foo]: ...

    @abstractmethod
    async def save(self, foo: Foo) -> Foo: ...
```

**`DDL` constant in `sqlite.py`:**

```python
DDL = """
CREATE TABLE IF NOT EXISTS foo (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    name      TEXT NOT NULL,
    created   TEXT NOT NULL
);
"""
```

### Error Handling

Raise subclasses of `CloudTwinError` from service code. Handlers catch these
and format the response appropriately for their protocol.

```python
from cloudtwin.core.errors import NotFoundError, ValidationError, IdentityNotVerifiedError

raise NotFoundError("Bucket not found: my-bucket")     # → HTTP 404
raise ValidationError("Invalid key format")             # → HTTP 400
raise IdentityNotVerifiedError("sender@example.com")    # → HTTP 400 (code MessageRejected)
```

---

## Supported Operations

The tables below list every operation currently implemented per service.

### AWS SES (v1 — Query protocol, `POST /`)

| Action | Description |
|---|---|
| `VerifyEmailIdentity` | Register an email address as a verified sender |
| `VerifyDomainIdentity` | Register a domain as a verified sender |
| `VerifyEmailAddress` | Alias for `VerifyEmailIdentity` (legacy) |
| `ListIdentities` | List all verified identities |
| `SendEmail` | Send an email from a verified identity |

### AWS SES v2 (REST/JSON, `POST /v2/...`)

| Endpoint | Description |
|---|---|
| `POST /v2/email/identities` | Create an email identity |
| `GET /v2/email/identities` | List all email identities |
| `GET /v2/email/identities/{identity}` | Get a single identity |
| `DELETE /v2/email/identities/{identity}` | Delete an identity |
| `POST /v2/email/outbound-emails` | Send an email |

### AWS S3 (REST, `/{bucket}/...`)

| Method + Path | Description |
|---|---|
| `GET /` | List all buckets |
| `PUT /{bucket}` | Create a bucket |
| `GET /{bucket}` | List objects in a bucket |
| `DELETE /{bucket}` | Delete a bucket |
| `PUT /{bucket}/{key}` | Upload an object |
| `GET /{bucket}/{key}` | Download an object |
| `DELETE /{bucket}/{key}` | Delete an object |

### AWS SNS (Query protocol, `POST /`)

| Action | Description |
|---|---|
| `CreateTopic` | Create an SNS topic |
| `DeleteTopic` | Delete a topic |
| `ListTopics` | List all topics |
| `Subscribe` | Subscribe an endpoint to a topic |
| `Unsubscribe` | Remove a subscription |
| `ListSubscriptions` | List all subscriptions |
| `Publish` | Publish a message to a topic |

### AWS SQS (JSON protocol, `POST /`)

| Action | Description |
|---|---|
| `CreateQueue` | Create a queue |
| `ListQueues` | List all queues |
| `GetQueueUrl` | Get the URL for a named queue |
| `SendMessage` | Send a message to a queue |
| `ReceiveMessage` | Poll and receive messages |
| `DeleteMessage` | Delete a received message by receipt handle |
| `DeleteQueue` | Delete a queue |
| `ChangeMessageVisibility` | Change the visibility timeout of a message |
| `GetQueueAttributes` | Get queue attributes |

### AWS DynamoDB (JSON protocol, `POST /`)

| Action | Description |
|---|---|
| `CreateTable` | Create a table |
| `DescribeTable` | Get table details |
| `ListTables` | List all tables |
| `DeleteTable` | Delete a table |
| `PutItem` | Create or replace an item |
| `GetItem` | Get an item by primary key |
| `DeleteItem` | Delete an item |
| `UpdateItem` | Update attributes on an existing item |
| `Scan` | Return all items, with optional filter |
| `Query` | Query items by key condition |
| `BatchWriteItem` | Batch put or delete items |
| `BatchGetItem` | Batch get items |

### AWS Secrets Manager (JSON protocol, `POST /`)

| Action | Description |
|---|---|
| `CreateSecret` | Create a new secret |
| `GetSecretValue` | Retrieve the current secret value |
| `PutSecretValue` | Store a new version of a secret |
| `DescribeSecret` | Get secret metadata |
| `ListSecrets` | List all secrets |
| `DeleteSecret` | Delete a secret |
| `UpdateSecret` | Update a secret's description or KMS key ID |

### AWS Lambda (REST, `/2015-03-31/functions/...`)

| Method + Path | Description |
|---|---|
| `POST /2015-03-31/functions` | Create a function |
| `GET /2015-03-31/functions` | List all functions |
| `GET /2015-03-31/functions/{name}` | Get function configuration |
| `PUT /2015-03-31/functions/{name}/code` | Update function code |
| `DELETE /2015-03-31/functions/{name}` | Delete a function |
| `POST /2015-03-31/functions/{name}/invocations` | Invoke a function |

### Azure Blob Storage (REST, `/{container}/...`)

| Method + Path | Description |
|---|---|
| `PUT /{container}` | Create a container |
| `GET /{container}` | List blobs in a container |
| `DELETE /{container}` | Delete a container |
| `PUT /{container}/{blob}` | Upload a blob |
| `GET /{container}/{blob}` | Download a blob |
| `DELETE /{container}/{blob}` | Delete a blob |

### Azure Service Bus (REST, `/{namespace}/...`)

| Method + Path | Description |
|---|---|
| `PUT /{ns}/queues/{queue}` | Create a queue |
| `GET /{ns}/queues` | List queues |
| `GET /{ns}/queues/{queue}` | Get queue details |
| `DELETE /{ns}/queues/{queue}` | Delete a queue |
| `POST /{ns}/queues/{queue}/messages` | Send a message |
| `GET /{ns}/queues/{queue}/messages` | Receive (lock) messages |
| `DELETE /{ns}/queues/{queue}/messages/{lock_token}` | Complete a message |
| `POST /{ns}/queues/{queue}/messages/{lock_token}/abandon` | Abandon a message |
| `PUT /{ns}/topics/{topic}` | Create a topic |
| `GET /{ns}/topics` | List topics |
| `DELETE /{ns}/topics/{topic}` | Delete a topic |
| `PUT /{ns}/topics/{topic}/subscriptions/{sub}` | Create a subscription |
| `GET /{ns}/topics/{topic}/subscriptions` | List subscriptions |
| `POST /{ns}/topics/{topic}/messages` | Publish to a topic (fan-out) |
| `GET /{ns}/topics/{topic}/subscriptions/{sub}/messages` | Receive from a subscription |
| `DELETE /{ns}/topics/{topic}/subscriptions/{sub}/messages/{lock_token}` | Complete a subscription message |

### Azure Queue Storage (REST, `/azure/queue/...`)

| Method + Path | Description |
|---|---|
| `PUT /azure/queue/{account}/{queue}` | Create a queue |
| `GET /azure/queue/{account}` | List queues |
| `POST /azure/queue/{account}/{queue}/messages` | Enqueue a message |
| `GET /azure/queue/{account}/{queue}/messages` | Dequeue messages |
| `GET /azure/queue/{account}/{queue}/messages/peek` | Peek at messages |
| `DELETE /azure/queue/{account}/{queue}/messages/{id}` | Delete a message |
| `DELETE /azure/queue/{account}/{queue}` | Delete a queue |

### Azure Event Grid (REST, `/azure/eventgrid/...`)

| Method + Path | Description |
|---|---|
| `PUT /azure/eventgrid/topics/{topic}` | Create a topic |
| `GET /azure/eventgrid/topics` | List topics |
| `DELETE /azure/eventgrid/topics/{topic}` | Delete a topic |
| `POST /azure/eventgrid/topics/{topic}/events` | Publish events to a topic |
| `GET /azure/eventgrid/topics/{topic}/events` | Get published events |

### Azure Key Vault (REST, `/azure/keyvault/...`)

| Method + Path | Description |
|---|---|
| `PUT /azure/keyvault/{vault}/secrets/{name}` | Set a secret |
| `GET /azure/keyvault/{vault}/secrets` | List secrets |
| `GET /azure/keyvault/{vault}/secrets/{name}` | Get a secret |
| `GET /azure/keyvault/{vault}/secrets/{name}/{version}` | Get a specific version |
| `DELETE /azure/keyvault/{vault}/secrets/{name}` | Delete a secret |

### Azure Functions (REST, `/azure/functions/...`)

| Method + Path | Description |
|---|---|
| `PUT /azure/functions/{app}/functions/{name}` | Register a function |
| `GET /azure/functions/{app}/functions` | List functions |
| `GET /azure/functions/{app}/functions/{name}` | Get function details |
| `POST /azure/functions/{app}/functions/{name}/invoke` | Invoke a function |
| `DELETE /azure/functions/{app}/functions/{name}` | Delete a function |

### GCP Cloud Storage (REST, `/storage/v1/b/...`)

| Method + Path | Description |
|---|---|
| `POST /storage/v1/b` | Create a bucket |
| `GET /storage/v1/b` | List buckets |
| `GET /storage/v1/b/{bucket}` | Get bucket metadata |
| `DELETE /storage/v1/b/{bucket}` | Delete a bucket |
| `POST /upload/storage/v1/b/{bucket}/o` | Upload an object |
| `PUT /upload/storage/v1/b/{bucket}/o` | Upload an object (resumable) |
| `GET /storage/v1/b/{bucket}/o` | List objects |
| `GET /storage/v1/b/{bucket}/o/{object}` | Get object metadata |
| `GET /download/storage/v1/b/{bucket}/o/{object}` | Download an object |
| `DELETE /storage/v1/b/{bucket}/o/{object}` | Delete an object |

### GCP Pub/Sub (REST, `/v1/projects/...`)

| Method + Path | Description |
|---|---|
| `PUT /v1/projects/{project}/topics/{topic}` | Create a topic |
| `GET /v1/projects/{project}/topics` | List topics |
| `GET /v1/projects/{project}/topics/{topic}` | Get topic details |
| `DELETE /v1/projects/{project}/topics/{topic}` | Delete a topic |
| `POST /v1/projects/{project}/topics/{topic}:publish` | Publish messages |
| `PUT /v1/projects/{project}/subscriptions/{sub}` | Create a subscription |
| `GET /v1/projects/{project}/subscriptions` | List subscriptions |
| `GET /v1/projects/{project}/subscriptions/{sub}` | Get subscription details |
| `DELETE /v1/projects/{project}/subscriptions/{sub}` | Delete a subscription |
| `POST /v1/projects/{project}/subscriptions/{sub}:pull` | Pull messages |
| `POST /v1/projects/{project}/subscriptions/{sub}:acknowledge` | Acknowledge messages |

### GCP Firestore (REST, `/v1/projects/.../documents/...`)

| Method + Path | Description |
|---|---|
| `POST /v1/projects/{project}/databases/(default)/documents/{collection}` | Create a document |
| `GET /v1/projects/{project}/databases/(default)/documents/{collection}` | List documents |

### GCP Cloud Tasks (REST, `/v2/projects/.../queues/...`)

| Method + Path | Description |
|---|---|
| `POST /v2/projects/{project}/locations/{loc}/queues` | Create a queue |
| `GET /v2/projects/{project}/locations/{loc}/queues` | List queues |
| `GET /v2/projects/{project}/locations/{loc}/queues/{queue}` | Get queue details |
| `DELETE /v2/projects/{project}/locations/{loc}/queues/{queue}` | Delete a queue |
| `GET /v2/projects/{project}/locations/{loc}/queues/{queue}/tasks` | List tasks in a queue |

### GCP Secret Manager (REST, `/v1/projects/.../secrets/...`)

| Method + Path | Description |
|---|---|
| `POST /v1/projects/{project}/secrets` | Create a secret |
| `GET /v1/projects/{project}/secrets` | List secrets |
| `POST /v1/projects/{project}/secrets/{name}:addVersion` | Add a new version |
| `DELETE /v1/projects/{project}/secrets/{name}` | Delete a secret |

### GCP Cloud Functions (REST, `/v2/projects/.../functions/...`)

| Method + Path | Description |
|---|---|
| `POST /v2/projects/{project}/locations/{loc}/functions` | Create a function |
| `GET /v2/projects/{project}/locations/{loc}/functions` | List functions |
| `GET /v2/projects/{project}/locations/{loc}/functions/{name}` | Get function details |
| `DELETE /v2/projects/{project}/locations/{loc}/functions/{name}` | Delete a function |

---

## Adding a New AWS Service

Example: adding Kinesis.

1. **Create the provider package** at `src/cloudtwin/providers/aws/kinesis/`:
   - `__init__.py` — must define `register(app, config, repos, telemetry, *, query_router, json_router) -> None`
   - `service.py` — pure business logic; depends on repos + telemetry only
   - `handlers.py` — HTTP layer; registers actions into the appropriate router

2. **Create the repository package** at `src/cloudtwin/persistence/repositories/aws/kinesis/`:
   - `repository.py` — abstract interface(s)
   - `sqlite.py` — SQLite implementation(s) **plus a module-level `DDL` string constant**
   - `__init__.py` — re-exports all classes

3. **Register the DDL** — import the new `DDL` in `persistence/db.py` and add it to the `DDL` concatenation.

4. **Add model dataclasses** to `persistence/models/aws/kinesis.py`.

5. **Wire the repository** — add the new key(s) to `make_repositories()` in `persistence/repositories/__init__.py`.

6. **Register the service** — add `"kinesis"` to `_SERVICE_REGISTRY` in `providers/aws/provider.py`.

7. **Add to config** — add `"kinesis"` to the services list in `config/cloudtwin.yml` and to the default in `config.py`.

8. **Add integration tests** under `tests/integration/providers/aws/`. Add a boto3 fixture to `conftest.py` and the service name to the `services=[...]` list in the `server_url` fixture.

**Which router to use:**

| Protocol | Content-Type | Registration |
|---|---|---|
| Query (e.g. SNS) | `application/x-www-form-urlencoded` | `query_router.register("ActionName", handler)` |
| JSON (e.g. SQS) | `application/x-amz-json-1.0` | `json_router.register("Service.ActionName", handler)` |
| REST (e.g. S3) | varies | FastAPI path routing via `app` directly |

`provider.py` itself does **not** need any other changes.

---

## Adding a New Cloud Provider

1. Create `src/cloudtwin/providers/<provider>/provider.py` with a
   `<Provider>Provider` class that implements `register(app)` using the same
   service-discovery pattern as `AwsProvider`. The existing `AzureProvider`
   and `GcpProvider` are the clearest examples to follow.
2. Add service packages under `providers/<provider>/`.
3. Add repository packages under `persistence/repositories/<provider>/`.
4. Wire the new provider in `app.py` alongside `AwsProvider`, `AzureProvider`,
   and `GcpProvider`.

---

## AWS Protocol Reference

`AwsProvider` owns the single `POST /` endpoint and dispatches by `Content-Type`:

- `application/x-amz-json-1.0` → `JsonProtocolRouter` (SQS)
- anything else → `QueryProtocolRouter` (SES v1, SNS)

Both routers are created by `AwsProvider` and passed to each service's
`register()` via keyword args (`query_router=`, `json_router=`).

| Service | Protocol | Content-Type | Dispatch | Endpoint |
|---|---|---|---|---|
| SES v1 | AWS Query | `application/x-www-form-urlencoded` | `Action` form field | `POST /` |
| SNS | AWS Query | `application/x-www-form-urlencoded` | `Action` form field | `POST /` |
| SQS | AWS JSON | `application/x-amz-json-1.0` | `X-Amz-Target` header | `POST /` |
| SES v2 | REST/JSON | `application/json` | FastAPI path routing | `POST /v2/...` |
| S3 | REST | varies | FastAPI path routing | `PUT/GET/DELETE /{bucket}/...` |

---

## Configuration Internals

`load_config()` merges in the following order (highest priority first):

1. Environment variables (`CLOUDTWIN_*`)
2. YAML file at `CLOUDTWIN_CONFIG_PATH` (default `/config/cloudtwin.yml`)
3. Built-in defaults in `config.py`

Access config inside a service or handler via the `Config` dataclass passed to
`register()`. Never read environment variables directly in service or handler code.

---

## Running Tests

Integration tests use a real uvicorn server in in-memory mode and the official
boto3/Azure/GCP SDK clients — no mocks.

```bash
# Run all integration tests (no external services required)
make test-integration

# or directly
python -m pytest tests/integration/ -q
```

Current status: **126 passing** (79 AWS · 25 Azure · 22 GCP)

### Writing new integration tests

Use `StorageConfig(mode="memory")` — never create SQLite files in tests.
The session-scoped server shares state across all tests; use unique resource
names per test to avoid cross-test interference.

```python
# conftest.py: add a fixture for your new service
@pytest.fixture(scope="session")
def kinesis(server_url):
    import boto3
    return boto3.client("kinesis", endpoint_url=server_url, **_FAKE_CREDS)
```

Also add the service name to the `services=[...]` list in the `server_url` fixture.

### Coding conventions

---

## CloudTwin Lite (Rust)

The `rust/cloudtwin-lite/` directory contains a Rust reimplementation of CloudTwin.
The goal is a single statically-linked binary with no Python runtime dependency —
smaller, faster cold-starts, and easier to embed in CI containers.

> **Status:** Early development. Currently only S3 is implemented. Not at feature
> parity with the Python version. The Python build remains the primary runtime until
> parity is reached.

### Directory layout

```
rust/cloudtwin-lite/
  Cargo.toml          # Package manifest and dependencies
  src/
    main.rs           # Entry point — config, DB init, Axum router setup
    config.rs         # Config struct populated from environment variables
    db.rs             # SQLite connection pool (tokio-rusqlite) + migrations
    s3/
      mod.rs          # Re-exports router()
      handlers.rs     # Axum HTTP handlers — S3 REST endpoints
      service.rs      # S3 business logic (pure Rust, no HTTP)
      models.rs       # S3Bucket, S3Object structs
```

### Stack

| Concern | Crate |
|---|---|
| HTTP server | `axum` 0.7 |
| Async runtime | `tokio` |
| SQLite | `rusqlite` (bundled) + `tokio-rusqlite` |
| Serialisation | `serde` + `serde_json` |
| Logging | `tracing` + `tracing-subscriber` |

### Architecture

The Lite binary follows the same service/handler separation as the Python version:

- `handlers.rs` — HTTP only. Parses request, calls service, formats XML response.
- `service.rs` — Business logic only. No HTTP, no XML. Calls the DB directly.
- `db.rs` — Connection pool and DDL migrations. Each service module owns its own
  `migrate()` call that runs `CREATE TABLE IF NOT EXISTS` statements.

Shared application state (`AppState`) is wrapped in `Arc` and injected into every
handler via Axum's `State` extractor.

### Configuration

Configuration is resolved from environment variables at startup — no YAML file
support yet.

| Variable | Default | Description |
|---|---|---|
| `CLOUDTWIN_PORT` | `4793` | TCP port |
| `CLOUDTWIN_DB_PATH` | `/data/cloudtwin-lite.db` | SQLite file path; use `:memory:` for ephemeral |

### Building

```bash
cd rust/cloudtwin-lite

# Debug
cargo build

# Release (optimised, ~8 MB binary)
cargo build --release

# Run
./target/release/cloudtwin-lite
```

### Adding a new service to Lite

1. Create `src/<service>/` with `mod.rs`, `handlers.rs`, `service.rs`, `models.rs`.
2. Add the migration SQL to a `migrate()` async fn in `service.rs` (or `db.rs`) and
   call it from `main.rs` after `db.migrate()`.
3. Expose a `pub fn router(state: Arc<AppState>) -> Router` from `mod.rs`.
4. Merge the router in `main.rs`: `app = app.merge(<service>::router(Arc::clone(&state)))`.
5. Add the module declaration in `main.rs`: `mod <service>;`.

### Known gaps vs Python version

- Only S3 is implemented (no SES, SNS, SQS, DynamoDB, Azure, or GCP services)
- No dashboard support
- No YAML config file — env vars only
- No telemetry / event log
- No Docker image published yet
- No integration test suite

- `from __future__ import annotations` at the top of every file
- Async throughout — all repository methods and service methods are `async`
- Dataclasses for all models — no ORM, no Pydantic in the persistence layer
- `__init__.py` files are for re-exports and the `register()` entry point only —
  never for instantiation or side effects
- Logger names follow the module hierarchy: `logging.getLogger("cloudtwin.ses")`
- ISO 8601 UTC strings for all timestamps: `datetime.now(timezone.utc).isoformat()`

---

## Dashboard

The dashboard is served by the main FastAPI app on the same API port as the
cloud endpoints. The browser entrypoint is `/dashboard`, static assets are under
`/dashboard/static/...`, and the backing JSON endpoints live under
`/api/dashboard/*`.

The static UI is located under `dashboard/static/` in the repo root.

### Directory layout

```
dashboard/
  static/
    index.html
    js/
      api.js              # Thin client for /api/dashboard/*
      main.js             # App bootstrap
      router.js           # Hash-based client-side router
      pages/              # Per-page renderers
```

### API contract

The dashboard calls `/api/dashboard/*` endpoints. Those routes are registered by
the Python backend in `src/cloudtwin/api/dashboard/` and are available through
the same server process as the rest of CloudTwin.

| Dashboard route | Expected API endpoint |
|---|---|
| Overview | `GET /api/dashboard/health` |
| Event Log | `GET /api/dashboard/events?limit=N` |
| AWS SES | `GET /api/dashboard/aws/ses` |
| AWS S3 | `GET /api/dashboard/aws/s3` |
| AWS SNS | `GET /api/dashboard/aws/sns` |
| AWS SQS | `GET /api/dashboard/aws/sqs` |
| Azure Blob | `GET /api/dashboard/azure/blob` |
| Azure Service Bus | `GET /api/dashboard/azure/servicebus` |
| GCP Storage | `GET /api/dashboard/gcp/storage` |
| GCP Pub/Sub | `GET /api/dashboard/gcp/pubsub` |

### Enabling / disabling

`DashboardConfig.enabled` controls whether the browser UI is mounted at
`/dashboard`. The dashboard API router is still registered so the backend can
serve `/api/dashboard/*` consistently and support local UI development.

Current default: `true`.

Users can still override it via config or env var:

```yaml
cloudtwin:
  dashboard:
    enabled: true
```

```bash
CLOUDTWIN_DASHBOARD_ENABLED=true python -m cloudtwin
```

### Development workflow

```bash
# Terminal 1 — Python API + dashboard UI
python -m cloudtwin
```

Then open `http://localhost:4793/dashboard`.

### Production build

The Python backend serves the static dashboard from `dashboard/static/` via
FastAPI `StaticFiles` when the dashboard UI is enabled:

```python
# app.py
from fastapi.staticfiles import StaticFiles
if config.dashboard.enabled:
  app.mount("/dashboard/static", StaticFiles(directory="dashboard/static"), name="dashboard-static")
```
