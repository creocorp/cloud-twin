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

The tables below list every operation currently implemented per service in the
Python backend. CloudTwin Lite implements a superset for SES, S3, SNS, and SQS,
plus KMS and Cognito — see [CloudTwin Lite (Rust)](#cloudtwin-lite-rust).

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
| `POST /v2/email/list-identities` | List all email identities (route used by newer botocore) |
| `GET /v2/email/identities/{identity}` | Get a single identity |
| `DELETE /v2/email/identities/{identity}` | Delete an identity |
| `POST /v2/email/outbound-emails` | Send an email |

### Mailbox — SMTP capture (CloudTwin Lite only, `smtp://:1025`)

A built-in SMTP sink that captures mail for local inspection, implemented in
`rust/cloudtwin-lite/src/mailbox/`. `smtp.rs` is a minimal hand-rolled SMTP
state machine (EHLO/HELO, MAIL, RCPT, DATA, RSET, NOOP, QUIT; no TLS/auth, no
relay). `service.rs::capture_raw` is the single ingestion path: it parses the
RFC 822 message with `mail-parser` and writes to `mailbox_messages` /
`mailbox_attachments`. SES v1/v2 `SendEmail` and `SendRawEmail` also capture
through it, so sent mail shows up in the same inbox. Before capture, each
message runs through the active SES receipt rule set; recipients matched by an
enabled rule are handled by its actions instead (see
[AWS SES (Lite)](#aws-ses-lite)).

| Setting | YAML | Env | Default |
|---|---|---|---|
| Enable | `mailbox.enabled` | `CLOUDTWIN_MAILBOX_ENABLED` | `false` (shipped config: `true`) |
| Port | `mailbox.smtp_port` | `CLOUDTWIN_SMTP_PORT` | `1025` |

| Endpoint | Description |
|---|---|
| `GET /api/dashboard/mailbox` | List captured messages (newest first, max 200) |
| `GET /api/dashboard/mailbox/{id}` | Full message detail including attachment metadata |

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

### AWS Bedrock (simulation, REST)

Bedrock is a simulation engine — it does not call real models. All responses are
generated deterministically from configuration.

| Method + Path | SDK operation | Description |
|---|---|---|
| `GET /foundation-models` | `bedrock.list_foundation_models()` | List known model stubs |
| `POST /model/{modelId}/invoke` | `bedrock-runtime.invoke_model()` | Invoke a model (non-streaming) |
| `POST /model/{modelId}/invoke-with-response-stream` | `bedrock-runtime.invoke_model_with_response_stream()` | Invoke a model with EventStream streaming |

**NOTE:** Bedrock's routes must be registered before S3's wildcard
`GET /{bucket}` route. In the `services` list, always place `"bedrock"` before
`"s3"`.

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

## AWS Bedrock Simulation

CloudTwin includes a fully config-driven Bedrock simulation engine for testing
application-level Bedrock integration logic without calling real models.

### Module layout

```
src/cloudtwin/providers/aws/bedrock/
  __init__.py         # register() entry point
  models.py           # Config dataclasses (BedrockSimConfig, ModelSimConfig, …)
  state.py            # BedrockState – thread-safe per-model request counters
  generator.py        # BedrockGenerator – deterministic text/schema/static generation
  scenario_engine.py  # ScenarioEngine – central response-resolution pipeline
  streaming.py        # EventStream binary encoding + async chunk generator
  handlers.py         # REST HTTP handlers (InvokeModel, InvokeModelWithResponseStream, ListFoundationModels)
```

### Response-resolution order

For every request the engine follows this strict order:

1. Resolve model config (fall back to global defaults for unknown models)
2. Increment per-model request counter
3. Check configured injected errors (`every: N`)
4. Check prompt rules (`contains` — first match wins)
5. Check sequence / cycle responses
6. Fallback to configured mode (`text`, `schema`, `static`)
7. Attach latency config
8. For streaming endpoint: wrap resolved body in EventStream chunks

### Full YAML reference

```yaml
bedrock:
  defaults:
    mode: text            # "text" | "schema" | "static"
    latency:
      min_ms: 0
      max_ms: 0

  models:
    my.model:
      mode: text          # response mode for fallback
      text:               # text-mode options
        template: lipsum
        min_words: 5
        max_words: 20

      schema:             # schema-mode: generate from JSON Schema
        type: object
        properties:
          answer:
            type: string
          score:
            type: number
          tags:
            type: array
            items:
              type: string

      static:             # static-mode: return verbatim
        result: fixed
        value: 42

      sequence:           # ordered or cycled multi-response
        mode: sequence    # "sequence" or "cycle"
        responses:
          - static:
              answer: first
          - static:
              answer: second

      rules:              # prompt-based matching (contains, first match wins)
        - contains: "sentiment"
          response:
            static:
              sentiment: positive
        - contains: "fail"
          error:
            type: ThrottlingException
            message: "Rule-triggered throttle"

      errors:             # periodic error injection
        - every: 5
          type: ThrottlingException
          message: "Every 5th request fails"

      streaming:          # for invoke-with-response-stream
        enabled: true
        chunk_mode: word           # "word" | "char" | "fixed_chars"
        fixed_chunk_size: 10      # used when chunk_mode = "fixed_chars"
        first_chunk_delay_ms: 0
        chunk_delay_ms: 0
        fail_after_chunks: null   # set to N to simulate mid-stream failure

      latency:
        min_ms: 50
        max_ms: 150
```

### Schema types supported

| JSON Schema type | Generated value |
|---|---|
| `string` | Two random lorem-ipsum words |
| `number` | Deterministic float from `[0.42, 0.73, …]` cycled by request count |
| `integer` | Deterministic integer from `[1, 7, 42, …]` cycled by request count |
| `boolean` | `true` on odd request counts, `false` on even |
| `array` | 1–3 items of the configured `items` type |
| `object` | Recurse into `properties` |
| `enum` | Deterministically rotates through enum values |

All values are seeded from `(model_id, field_path, request_count)` for stability.

### Testing with boto3

```python
import boto3, json
from botocore.config import Config as BotocoreConfig

# Disable auto-retry so simulated ThrottlingException errors are not silently
# swallowed and retried before reaching the test.
client = boto3.client(
    "bedrock-runtime",
    endpoint_url="http://localhost:4793",
    aws_access_key_id="test",
    aws_secret_access_key="test",
    region_name="us-east-1",
    config=BotocoreConfig(retries={"total_max_attempts": 1, "mode": "standard"}),
)

# Non-streaming
resp = client.invoke_model(
    modelId="my.model",
    body=json.dumps({"prompt": "what is the sentiment?"}).encode(),
    contentType="application/json",
    accept="application/json",
)
result = json.loads(resp["body"].read())

# Streaming
resp = client.invoke_model_with_response_stream(
    modelId="my.model",
    body=json.dumps({"prompt": "explain this"}).encode(),
    contentType="application/json",
)
for event in resp["body"]:
    chunk = event.get("chunk")
    if chunk:
        data = json.loads(chunk["bytes"].decode())
        print(data.get("delta", {}).get("text", ""), end="", flush=True)
```

### Caveats

- This is a simulator, not a real model. Response quality is synthetic.
- Behavior is deterministic by design — same input, same counter, same output.
- Provider-specific payload shapes (Anthropic, Meta, Amazon Titan, etc.) are
  not reproduced exactly. Use the generic `content` / `stop_reason` fields.
- `ListFoundationModels` returns a static catalogue of common model IDs.

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

- `from __future__ import annotations` at the top of every file
- Async throughout — all repository methods and service methods are `async`
- Dataclasses for all models — no ORM, no Pydantic in the persistence layer
- `__init__.py` files are for re-exports and the `register()` entry point only —
  never for instantiation or side effects
- Logger names follow the module hierarchy: `logging.getLogger("cloudtwin.ses")`
- ISO 8601 UTC strings for all timestamps: `datetime.now(timezone.utc).isoformat()`

---

## CloudTwin Lite (Rust)

`rust/cloudtwin-lite/` is a single statically linked binary with no Python
runtime. It is the runtime going forward: new AWS features (SES receiving and
events, S3 notifications/multipart/versioning, worker-grade SQS, SNS fan-out,
KMS, Cognito) exist only here, and the Python backend is being phased out.

### Directory layout

```
rust/cloudtwin-lite/
  Cargo.toml
  src/
    main.rs            # Entry point — load config, build state, seed, serve HTTP + SMTP
    app.rs             # AppState, build_router(), AWS `POST /` dispatcher, virtual-host rewrite
    config.rs          # YAML + env config (module configs live in each module)
    db.rs              # SQLite connection, shared DDL, ensure_column(), cloudtwin_meta
    proto.rs           # AWS wire helpers: Query/JSON parsing, XML/JSON errors, query_to_json
    xml.rs             # Minimal XML DOM for request bodies (S3 configs, multipart, ...)
    control.rs         # /_cloudtwin/reset and /_cloudtwin/seed
    seed.rs            # Seed document dispatcher (startup, POST /_cloudtwin/seed, reset)
    telemetry.rs       # events table writer
    testutil.rs        # In-process test harness (cfg(test))
    mailbox/           # SMTP listener + capture inbox
    aws/
      <service>/       # mod.rs · handlers.rs · service.rs · models.rs · schema.rs · tests.rs
                       # (+ config.rs where the service is configurable)
      ses/  s3/  sns/  sqs/  kms/  cognito/  dynamodb/  secretsmanager/  bedrock/
    azure/  gcp/
```

### Architecture

- **`handlers.rs`** — protocol only (Query/XML, JSON, REST). Parses requests,
  calls the service, maps typed errors to AWS error codes.
- **`service.rs`** — business logic only; talks to SQLite through `state.db.call(...)`.
- **`schema.rs`** — each module owns its tables: `CREATE TABLE IF NOT EXISTS`
  plus `db::ensure_column` for columns added later, so old database files
  upgrade in place. Table names start with the module prefix (`ses_`, `s3_`,
  `sqs_`, `sns_`, `kms_`, `cognito_`) so `/_cloudtwin/reset` can find them.
- **Cross-service calls** go through small internal APIs rather than HTTP:
  `aws::sqs::send_internal`, `aws::sns::publish_internal`,
  `aws::s3::put_object_internal`, `mailbox::service::capture_raw`. This is how
  SNS fans out to SQS, S3 emits event notifications, SES receipt rules write to
  S3, and SES/Cognito drop mail in the inbox.
- **`seed(state, doc)`** — each module provisions its own section of a seed
  document idempotently.

`POST /` routes JSON-protocol requests by `X-Amz-Target` prefix (`AmazonSQS.`,
`DynamoDB_`, `secretsmanager.`, `TrentService.` for KMS,
`AWSCognitoIdentityProviderService.`) and form-encoded Query requests by
`Action` (SES v1, then SNS). Both `application/x-amz-json-1.0` and `-1.1` are
accepted.

### Configuration

Lite reads the shared `cloudtwin.yml` (`CLOUDTWIN_CONFIG_PATH`); env vars win.

| YAML | Env | Default | Purpose |
|---|---|---|---|
| `api_port` | `CLOUDTWIN_PORT` | `4793` | HTTP port |
| `storage.path` | `CLOUDTWIN_DB_PATH` | `/data/cloudtwin-lite.db` | SQLite file (`:memory:` for ephemeral) |
| — | `CLOUDTWIN_BASE_URL` | `http://localhost:{port}` | Public URL used in queue URLs, links, OIDC issuer |
| `mailbox.enabled` / `mailbox.smtp_port` | `CLOUDTWIN_MAILBOX_ENABLED` / `CLOUDTWIN_SMTP_PORT` | `false` / `1025` | SMTP listener (capture inbox + SES receiving) |
| `seed:` | `CLOUDTWIN_SEED_PATH` | — | Resources to provision at startup (YAML/JSON file) |
| `providers.aws.ses.strict_verification` | `CLOUDTWIN_SES_STRICT_VERIFICATION` | `true` | Reject sends from unverified identities |
| `providers.aws.ses.verification.mode` | `CLOUDTWIN_SES_VERIFICATION_MODE` | `auto` | `auto`, `manual`, `after_checks`, `delay` |
| `providers.aws.ses.verification.checks` | `CLOUDTWIN_SES_VERIFICATION_CHECKS` | `3` | Reads before `after_checks` resolves |
| `providers.aws.ses.verification.delay_seconds` | `CLOUDTWIN_SES_VERIFICATION_DELAY_SECONDS` | `30` | Delay before `delay` resolves |
| `providers.aws.ses.verification.outcome` | `CLOUDTWIN_SES_VERIFICATION_OUTCOME` | `SUCCESS` | `SUCCESS`, `FAILED`, `TEMPORARY_FAILURE` |
| `providers.aws.ses.verification.overrides` | — | `{}` | Per-identity rules (exact or `*.domain` / `*@domain`) |
| `providers.aws.ses.account.production_access_enabled` | `CLOUDTWIN_SES_PRODUCTION_ACCESS` | `true` | `false` = sandbox (recipients must be verified) |
| `providers.aws.ses.account.sending_enabled` | `CLOUDTWIN_SES_SENDING_ENABLED` | `true` | Account-level sending switch |
| `providers.aws.ses.account.{max_24_hour_send,max_send_rate,enforcement_status,suppressed_reasons}` | — | `50000`, `14`, `HEALTHY`, `[BOUNCE, COMPLAINT]` | `GetAccount` values |
| `providers.aws.s3.virtual_host_suffix` | `CLOUDTWIN_S3_VIRTUAL_HOST_SUFFIX` | `s3.localhost` | `{bucket}.s3.localhost:4793` → path style; empty disables |
| `providers.aws.cognito.issuer_base_url` | `CLOUDTWIN_COGNITO_ISSUER_BASE_URL` | `base_url` | OIDC issuer prefix |
| `providers.aws.cognito.signing_key_path` | `CLOUDTWIN_COGNITO_SIGNING_KEY_PATH` | generated + persisted | RSA PEM for RS256 tokens |
| `providers.aws.cognito.{id,access,refresh}_token_ttl_seconds` | `CLOUDTWIN_COGNITO_*_TTL_SECONDS` | 3600 / 3600 / 30 days | Defaults when a client sets none |

### Test control plane (`/_cloudtwin`)

CloudTwin-specific endpoints for integration tests (not AWS APIs):

| Endpoint | Purpose |
|---|---|
| `POST /_cloudtwin/reset` | Clear state. Body (optional): `{"services": ["ses","s3","sqs",...], "reseed": true}`. Services: `s3 ses sqs sns kms cognito dynamodb secretsmanager mailbox events azure gcp`. The startup seed is re-applied unless `reseed: false`. Signing keys survive. |
| `POST /_cloudtwin/seed` | Apply a seed document (same shape as YAML `seed:`) |
| `POST /_cloudtwin/ses/identities/{identity}/verification` | Force `{"status": "SUCCESS|PENDING|FAILED|TEMPORARY_FAILURE", "mail_from_status": ...}` |
| `GET /_cloudtwin/ses/verify-email?identity=` | Verification link included in captured "Email Address Verification Request" mails |
| `GET|POST /_cloudtwin/ses/account` | Read / override `GetAccount` values (`null` reverts a key) |
| `GET /_cloudtwin/ses/events?message_id=&type=` | Every SES event emitted (Send, Delivery, Bounce, ...) |
| `GET /_cloudtwin/ses/messages` | Outbound messages with raw MIME and per-recipient outcome |
| `POST /_cloudtwin/ses/inbound?from=&to=a,b` | Deliver a raw RFC 822 body through the receipt rules (no SMTP needed) |

Seed documents combine module sections, applied in dependency order
(KMS → S3 → SQS → SNS → SES → Cognito):

```yaml
seed:
  s3:
    buckets: [mail-inbound]
  sqs:
    queues: [inbound-mail, ses-events]
  sns:
    topics:
      - name: ses-events
        subscriptions: [{protocol: sqs, endpoint: ses-events, attributes: {RawMessageDelivery: "true"}}]
  ses:
    identities: [example.com]
    configuration_sets:
      - name: outbound
        event_destinations:
          - {name: sns, matching_event_types: [SEND, DELIVERY, BOUNCE, COMPLAINT],
             sns_topic_arn: "arn:aws:sns:us-east-1:000000000000:ses-events"}
    receipt_rule_sets:
      - name: inbound
        active: true
        rules:
          - {Name: store, Enabled: true, Recipients: [example.com],
             Actions: [{S3Action: {BucketName: mail-inbound, ObjectKeyPrefix: raw/}}]}
```

### AWS SES (Lite)

**Identities.** `CreateEmailIdentity` / `GetEmailIdentity` return
`DkimAttributes` (three generated `Tokens`, `Status`,
`SigningAttributesOrigin: AWS_SES`, key lengths), `VerificationStatus`,
`VerificationInfo`, `MailFromAttributes`, `FeedbackForwardingStatus`, and
`ConfigurationSetName`. Domains verify every subdomain and address under them.
Creating an existing identity → `AlreadyExistsException`; reading/deleting a
missing one → `NotFoundException`. REST errors carry `x-amzn-ErrorType` and
`__type`, so boto3 and the Rust SDK raise typed exceptions.

**Verification lifecycle.** New identities follow
`providers.aws.ses.verification`: `auto` resolves immediately, `manual` stays
`PENDING` until the control endpoint is called, `after_checks` resolves after N
`GetEmailIdentity` (or v1 `GetIdentity*Attributes`) reads, `delay` after N
seconds. `outcome` picks the final status, `overrides` per identity. Pending
email identities get a verification email in the inbox whose link verifies them.

**Sending.** `SendEmail` (v1, v2) and `SendRawEmail` / v2 `Content.Raw`
(full MIME with attachments and threading headers), `ReplyToAddresses`,
`EmailTags`, `ConfigurationSetName`, `FeedbackForwardingEmailAddress`, and
v2 `Content.Simple.Headers`. Checks, in SES order: account sending enabled
(`SendingPausedException`), configuration set exists, sender verified
(`MessageRejected`, when `strict_verification`), sandbox recipients
(`production_access_enabled: false`), custom MAIL FROM status with
`REJECT_MESSAGE` (`MailFromDomainNotVerifiedException`), ≤ 50 recipients,
≤ 40 MB. Accepted mail lands in the local inbox (Bcc kept visible there).

**Events.** Configuration sets (`CreateConfigurationSet`, event destinations
CRUD, option PUTs; v1 equivalents) publish SES-shaped JSON events
(`eventType`, `mail`, and `send` / `delivery` / `bounce` / `complaint` /
`reject` / `deliveryDelay`) to SNS destinations. Every event is also logged at
`/_cloudtwin/ses/events`.

**Mailbox simulator.** `success@`, `bounce@` (Permanent/General),
`complaint@`, `suppressionlist@` (Permanent/Suppressed) and `ooto@` (delivery
plus an auto-reply) at `simulator.amazonses.com`, including `+label` variants.
CloudTwin extensions: `delay@` (DeliveryDelay) and `reject@` (Reject).
Bounces and complaints forward a feedback notification to the inbox unless
feedback forwarding is disabled for the identity.

**Suppression list.** `Put/Get/List/DeleteSuppressedDestination`; sends to a
listed address produce a `Bounce` event with `bounceSubType:
OnAccountSuppressionList` and are not delivered.

**Account.** `GetAccount` (`SendQuota` with real `SentLast24Hours`,
`ProductionAccessEnabled`, `SendingEnabled`, `EnforcementStatus`,
`SuppressionAttributes`), `PutAccountSendingAttributes`,
`PutAccountSuppressionAttributes`; v1 `GetSendQuota`,
`GetAccountSendingEnabled`, `UpdateAccountSendingEnabled`.

**Custom MAIL FROM.** `PutEmailIdentityMailFromAttributes` (must be a
subdomain of the identity), v1 `SetIdentityMailFromDomain` /
`GetIdentityMailFromDomainAttributes`. Its status follows the same
verification policy and can be forced via `mail_from_status`.

**Receiving.** v1 receipt rule sets and rules (`Create/Delete/List/Describe/
CloneReceiptRuleSet`, `Set/DescribeActiveReceiptRuleSet`,
`ReorderReceiptRuleSet`, `Create/Update/Delete/DescribeReceiptRule`,
`SetReceiptRulePosition`). Mail arriving on the SMTP listener (or
`POST /_cloudtwin/ses/inbound`) for recipients matched by an enabled rule in
the active set (`user@domain`, `domain`, `.subdomain`; no recipients = all)
runs the rule's actions; everything else goes to the capture inbox. Actions:
`S3Action` (writes `{prefix}{messageId}` with SES receiving headers prepended,
40 MB limit, optional SNS notification), `SNSAction` (UTF-8/Base64 content,
150 KB), `AddHeaderAction`, `BounceAction` (DSN to the inbox), `StopAction`;
`LambdaAction`/`WorkmailAction` only send their SNS notification. S3 writes
fire S3 event notifications, so `MX → receipt rule → S3 → SQS` works end to end.

```python
import boto3, smtplib
kw = dict(endpoint_url="http://localhost:4793", region_name="us-east-1",
          aws_access_key_id="test", aws_secret_access_key="test")
ses, sesv2, s3 = (boto3.client(n, **kw) for n in ("ses", "sesv2", "s3"))

dkim = sesv2.create_email_identity(EmailIdentity="example.com")["DkimAttributes"]["Tokens"]
s3.create_bucket(Bucket="mail-inbound")
ses.create_receipt_rule_set(RuleSetName="inbound")
ses.create_receipt_rule(RuleSetName="inbound", Rule={
    "Name": "store", "Enabled": True, "Recipients": ["example.com"],
    "Actions": [{"S3Action": {"BucketName": "mail-inbound", "ObjectKeyPrefix": "raw/"}}]})
ses.set_active_receipt_rule_set(RuleSetName="inbound")
with smtplib.SMTP("localhost", 1025) as smtp:      # mailbox.enabled: true
    smtp.sendmail("someone@else.net", ["team@example.com"], "Subject: hi\r\n\r\nhello")
```

### AWS S3 (Lite)

Path-style (`/{bucket}/{key}`, with or without the trailing slash the Rust SDK
sends) and virtual-hosted-style (`{bucket}.s3.localhost:4793`) addressing.

- **Buckets & listing:** bucket CRUD (`BucketNotEmpty` on non-empty delete),
  ListObjects V1/V2 (prefix, delimiter → `CommonPrefixes`, `max-keys`,
  continuation tokens / markers with real `IsTruncated`, `start-after`,
  `encoding-type=url`), ListObjectVersions, ListMultipartUploads.
- **Objects:** Put/Get/Head/Copy/Delete, `DeleteObjects`, user metadata
  (`x-amz-meta-*`) and standard headers, ranged GET (206 / 416), conditional
  requests, object tagging (`?tagging`, `x-amz-tagging`), GetObjectAttributes.
- **Bucket settings:** versioning (version ids, delete markers, `?versionId=`),
  SSE headers (`AES256`, `aws:kms` + key id, bucket-key) echoed on every
  response, default encryption, Block Public Access, lifecycle, policy, CORS,
  ownership controls, tagging, website/replication/object-lock stored and echoed.
  Lifecycle rules are stored, not enforced; encryption is header-only.
- **Multipart:** Create/UploadPart/UploadPartCopy/Complete/Abort/ListParts with
  AWS-style `"md5-of-md5s-N"` ETags.
- **Browser uploads:** CORS preflight (`OPTIONS`) and CORS response headers;
  presigned GET/PUT URLs; presigned POST policies (`${filename}`,
  `content-length-range`, `eq`/`starts-with`, `success_action_*`).
- **SDK streaming:** `Content-Encoding: aws-chunked` bodies with trailing
  checksums (Rust SDK, newer boto3) are decoded; CRC32/SHA256/Content-MD5 are
  validated and `x-amz-checksum-*` echoed.
- **Event notifications:** `PutBucketNotificationConfiguration` with queue and
  topic configurations (prefix/suffix filters). Destinations are validated and
  sent an `s3:TestEvent`, like AWS. `ObjectCreated:{Put,Post,Copy,CompleteMultipartUpload}`
  and `ObjectRemoved:{Delete,DeleteMarkerCreated}` records use the AWS shape
  (`Records[].s3.bucket.name`, `object.key` URL-encoded, `size`, `eTag`, `sequencer`).

```python
s3.put_bucket_notification_configuration(Bucket="uploads", NotificationConfiguration={
    "QueueConfigurations": [{"QueueArn": "arn:aws:sqs:us-east-1:000000000000:uploads-events",
                             "Events": ["s3:ObjectCreated:*"],
                             "Filter": {"Key": {"FilterRules": [{"Name": "prefix", "Value": "raw/"}]}}}]})
s3.put_object(Bucket="uploads", Key="raw/a.eml", Body=b"...", Metadata={"org": "o1"},
              ServerSideEncryption="aws:kms", SSEKMSKeyId="alias/mail")
post = s3.generate_presigned_post("uploads", "in/${filename}",
                                  Conditions=[["content-length-range", 1, 10_000_000]])
```

Seed: `s3: {buckets: [name | {name, versioning, cors: [CORSRule], notifications:
{queues: [{queue, events, prefix, suffix}], topics: [{topic, ...}]}, lifecycle,
public_access_block, encryption, policy, tags}]}` (seeding does not send
`s3:TestEvent`).

### AWS SQS & SNS (Lite)

**SQS** (AWS JSON, `X-Amz-Target: AmazonSQS.*`) supports queue attributes
(VisibilityTimeout, DelaySeconds, MessageRetentionPeriod, ReceiveMessageWaitTimeSeconds,
MaximumMessageSize, RedrivePolicy, Policy, KMS/SSE echo), FIFO queues (`.fifo` +
`FifoQueue=true`, MessageGroupId, 5-minute dedup, SequenceNumber, per-group ordering),
visibility timeouts with fresh receipt handles, long polling (WaitTimeSeconds ≤ 20),
dead-letter redrive after `maxReceiveCount`, message attributes with AWS-exact
`MD5OfMessageAttributes`, system attributes (`SentTimestamp`, `ApproximateReceiveCount`, ...),
batch APIs, SetQueueAttributes, tags and PurgeQueue. Errors include `x-amzn-query-error`,
so SDKs raise modeled exceptions (`QueueDoesNotExist`).

**SNS** (AWS Query) delivers on Publish/PublishBatch:
- `sqs`: standard notification JSON envelope, or the raw message when
  `RawMessageDelivery=true` (SNS message attributes become SQS message attributes).
- `http`/`https`: background POST of the envelope with `x-amz-sns-*` headers.
- `email`/`email-json`: captured in the local inbox.
- Subscriptions are **auto-confirmed** (`PendingConfirmation=false`); no confirmation
  message is sent. `FilterPolicy` (MessageAttributes or MessageBody scope) supports exact
  values, `prefix`, `suffix`, `anything-but`, `exists`, `numeric`, `$or`.
- Also: `MessageStructure=json`, `TargetArn`, Get/SetSubscriptionAttributes,
  Get/SetTopicAttributes, Unsubscribe, ListSubscriptions, tagging.

```python
dlq = sqs.create_queue(QueueName="orders-dlq")["QueueUrl"]
q = sqs.create_queue(QueueName="orders", Attributes={"RedrivePolicy": json.dumps(
    {"deadLetterTargetArn": "arn:aws:sqs:us-east-1:000000000000:orders-dlq", "maxReceiveCount": 3})})["QueueUrl"]
topic = sns.create_topic(Name="order-events")["TopicArn"]
sns.subscribe(TopicArn=topic, Protocol="sqs",
              Endpoint="arn:aws:sqs:us-east-1:000000000000:orders",
              Attributes={"RawMessageDelivery": "true",
                          "FilterPolicy": json.dumps({"type": ["created"]})})
sns.publish(TopicArn=topic, Message='{"id": 1}',
            MessageAttributes={"type": {"DataType": "String", "StringValue": "created"}})
msgs = sqs.receive_message(QueueUrl=q, WaitTimeSeconds=5, MessageAttributeNames=["All"])["Messages"]
```

Seed: `sqs: {queues: [name | {name, attributes, tags}]}` (`RedrivePolicy` may be a map and
`deadLetterTargetArn` a bare queue name); `sns: {topics: [name | {name, attributes, tags,
subscriptions: [{protocol, endpoint, attributes}]}]}` (a bare queue name is accepted as an
`sqs` endpoint).

### AWS KMS (Lite)

AWS JSON 1.1 (`X-Amz-Target: TrentService.*`) on `POST /`. Symmetric keys hold real
256-bit AES-GCM material, so ciphertexts are genuine and tamper-evident; the
`EncryptionContext` is bound as AAD.

Operations: CreateKey, DescribeKey, ListKeys, Create/Update/Delete/ListAliases, Encrypt,
Decrypt, ReEncrypt, GenerateDataKey, GenerateDataKeyWithoutPlaintext, GenerateRandom,
Enable/DisableKey, Schedule/CancelKeyDeletion, Enable/Disable/GetKeyRotationStatus,
Tag/Untag/ListResourceTags, UpdateKeyDescription, Get/Put/ListKeyPolicies. Key id, key ARN,
`alias/name`, and alias ARN are accepted wherever `KeyId` is. Disabled keys raise
`DisabledException`, keys pending deletion `KMSInvalidStateException`. Asymmetric/HMAC keys can
be created and described but not used; policies are stored, not enforced.

```python
key_id = kms.create_key(Description="mail secrets")["KeyMetadata"]["KeyId"]
kms.create_alias(AliasName="alias/mail", TargetKeyId=key_id)
blob = kms.encrypt(KeyId="alias/mail", Plaintext=b"app-password",
                   EncryptionContext={"org": "o1"})["CiphertextBlob"]
kms.decrypt(CiphertextBlob=blob, EncryptionContext={"org": "o1"})["Plaintext"]
```

Seed: `kms: {keys: [{id?: <uuid>, alias? | aliases?, description?, enabled?, tags?}]}`.
Keys seeded with a fixed `id` derive their key material from that id, so ciphertexts survive
restarts and `/_cloudtwin/reset` (local development only — the material is not secret).

### AWS Cognito user pools (Lite)

Admin/user JSON API (`AWSCognitoIdentityProviderService.*` on `POST /`), OIDC discovery +
JWKS, a hosted login page, and the `/oauth2/*` endpoints. Tokens are RS256 JWTs with
Cognito's claim shapes — ID token: `sub`, `email`, `email_verified`, `cognito:username`,
`aud`, `token_use: id`, `nonce`; access token: `client_id` (no `aud`), `scope`,
`token_use: access`, `username` — so APIs validate them exactly like real Cognito tokens.

| Endpoint | Purpose |
|---|---|
| `GET /{poolId}/.well-known/openid-configuration` | OIDC discovery |
| `GET /{poolId}/.well-known/jwks.json` | Signing keys |
| `GET /oauth2/authorize`, `GET/POST /login` | Hosted UI (code + PKCE S256/plain, session cookie) |
| `POST /oauth2/token` | `authorization_code`, `refresh_token`, `client_credentials` |
| `GET/POST /oauth2/userInfo`, `POST /oauth2/revoke`, `GET /logout` | userinfo, revocation, sign-out (`client_id` + `logout_uri`) |

Issuer: `{issuer_base_url or base_url}/{poolId}`. The signing key is generated once and
stored in the DB (it survives `/_cloudtwin/reset`), or loaded from a PEM file.

API: pool/client/domain CRUD, AdminCreateUser (temporary password, invite email captured in
the inbox), AdminGetUser, ListUsers (`attr = "v"` / `attr ^= "v"`),
Admin{Disable,Enable,Delete}User, AdminSetUserPassword, AdminUserGlobalSignOut,
Admin{Update,Delete}UserAttributes, AdminConfirmSignUp, AdminResetUserPassword,
(Admin)InitiateAuth (USER_PASSWORD, ADMIN_USER_PASSWORD, REFRESH_TOKEN),
(Admin)RespondToAuthChallenge (NEW_PASSWORD_REQUIRED), GetUser, GlobalSignOut, RevokeToken,
ChangePassword, SignUp/ConfirmSignUp, ForgotPassword/ConfirmForgotPassword (codes emailed to
the inbox). Not supported: SRP, MFA (TOTP), groups, resource servers, federation, Lambda
triggers.

Seed a stable local pool (idempotent):

```yaml
seed:
  cognito:
    user_pools:
      - id: us-east-1_localpool
        name: orkystra
        username_attributes: [email]
        clients:
          - client_id: fixedclientid      # public client, code flow, openid/email/profile
            name: web
            callback_urls: [http://localhost:3000/callback]
            logout_urls: [http://localhost:3000/]
        users:
          - email: dev@example.com
            password: Passw0rd!
            attributes: { name: Dev User }
```

```python
cog = boto3.client("cognito-idp", **kw)
cog.admin_create_user(UserPoolId="us-east-1_localpool", Username="new@example.com",
                      DesiredDeliveryMediums=["EMAIL"])   # invite lands in the local mailbox
r = cog.initiate_auth(ClientId="fixedclientid", AuthFlow="USER_PASSWORD_AUTH",
                      AuthParameters={"USERNAME": "dev@example.com", "PASSWORD": "Passw0rd!"})
id_token = r["AuthenticationResult"]["IdToken"]
```

The static routes `/login`, `/logout`, and `/oauth2/*` shadow S3 buckets with those names,
and `GET /{x}/.well-known/{openid-configuration,jwks.json}` is answered by Cognito.

### Building and testing

```bash
make rust-build        # release build
make rust-run          # run with in-memory storage
make rust-test         # cargo test — in-process tests for every module
```

In-process tests use `crate::testutil::TestApp` (real router, in-memory DB)
and drive the app over HTTP exactly like an SDK. The boto3 integration suite
can also target a running Lite binary with
`CLOUDTWIN_TEST_URL=http://127.0.0.1:47930` (see `make rust-test-parity`).

### Adding a new service to Lite

1. Create `src/aws/<service>/` with `mod.rs`, `handlers.rs`, `service.rs`,
   `models.rs`, `schema.rs` (`pub fn migrate(conn)`), and `tests.rs`.
2. Call `<service>::schema::migrate(conn)` from `Database::migrate` in `db.rs`.
3. Wire the protocol: an `X-Amz-Target` prefix or Query `Action` list in
   `app.rs::aws_post`, or merge a REST router in `app::build_router`.
4. Expose `pub async fn seed(state, doc)` and add it to `seed.rs`; add the
   table prefix to `SERVICE_TABLES` in `control.rs`.
5. If other services need to call it, expose a small `*_internal` function
   from `mod.rs` instead of going through HTTP.

### Known gaps vs the Python version

- Lambda, Azure Queue/Event Grid/Key Vault/Functions, and GCP Firestore/Cloud
  Tasks/Secret Manager/Cloud Functions are not implemented in Lite yet.
- Request signatures are never validated (presigned URLs and POST policies
  work because of this).

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
