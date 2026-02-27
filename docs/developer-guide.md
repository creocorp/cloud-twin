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
- [Adding a new AWS service](#adding-a-new-aws-service)
- [Adding a new cloud provider](#adding-a-new-cloud-provider)
- [AWS protocol reference](#aws-protocol-reference)
- [Configuration internals](#configuration-internals)
- [Running tests](#running-tests)

---

## Architecture Overview

CloudTwin is a FastAPI application. On startup it instantiates one _provider_
per cloud (`AwsProvider`, `AzureProvider`, `GcpProvider`). Each provider
discovers its services and calls their `register()` function, which mounts
HTTP routes and wires up dependencies.

All persistence is handled through _repository_ abstractions backed by either
SQLite (`aiosqlite`) or plain Python dicts. Services depend only on repository
interfaces and never look at which backend is active.

```
Request → FastAPI Router → Handler (protocol adapter)
                              ↓
                          Service (business logic)
                              ↓
                          Repository (abstract)
                         ↙           ↘
              SQLiteRepo         InMemoryRepo
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
        ses/                    # repository.py · inmemory.py · sqlite.py (DDL) · __init__.py
        s3/                     # repository.py · inmemory.py · sqlite.py (DDL) · __init__.py
        sns/                    # repository.py · inmemory.py · sqlite.py (DDL) · __init__.py
        sqs/                    # repository.py · inmemory.py · sqlite.py (DDL) · __init__.py
      azure/
        blob/                   # repository.py · inmemory.py · sqlite.py (DDL) · __init__.py
        servicebus/             # repository.py · inmemory.py · sqlite.py (DDL) · __init__.py
      gcp/
        storage/                # repository.py · inmemory.py · sqlite.py (DDL) · __init__.py
        pubsub/                 # repository.py · inmemory.py · sqlite.py (DDL) · __init__.py
      common/
        events/                 # repository.py · inmemory.py · sqlite.py (DDL) · __init__.py

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

2. **SQLite as source of truth.** All services store state in SQLite.
   An in-memory mode (Python dicts) is also supported for CI and ephemeral testing.

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
   classes. Two concrete implementations exist: `SqliteXxxRepository` and
   `InMemoryXxxRepository`. Services never check which implementation is active.

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
- An `inmemory.py` with the in-memory implementation
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

## Adding a New AWS Service

Example: adding Kinesis.

1. **Create the provider package** at `src/cloudtwin/providers/aws/kinesis/`:
   - `__init__.py` — must define `register(app, config, repos, telemetry, *, query_router, json_router) -> None`
   - `service.py` — pure business logic; depends on repos + telemetry only
   - `handlers.py` — HTTP layer; registers actions into the appropriate router

2. **Create the repository package** at `src/cloudtwin/persistence/repositories/aws/kinesis/`:
   - `repository.py` — abstract interface(s)
   - `inmemory.py` — in-memory implementation(s)
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

- `from __future__ import annotations` at the top of every file
- Async throughout — all repository methods and service methods are `async`
- Dataclasses for all models — no ORM, no Pydantic in the persistence layer
- `__init__.py` files are for re-exports and the `register()` entry point only —
  never for instantiation or side effects
- Logger names follow the module hierarchy: `logging.getLogger("cloudtwin.ses")`
- ISO 8601 UTC strings for all timestamps: `datetime.now(timezone.utc).isoformat()`
