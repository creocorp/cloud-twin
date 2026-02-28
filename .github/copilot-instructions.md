# GitHub Copilot Instructions

## Project Overview

CloudTwin is a lightweight, self-contained multi-cloud local runtime. It emulates
core cloud services (AWS SES, S3, and future Azure/GCP equivalents) inside a
single process with no external dependencies. All state is persisted in SQLite or
kept in-memory. It is designed as a drop-in endpoint override for SDK-based
application code during local development and CI.

**Ports:** `4793` → Cloud API, `8793` → Dashboard (opt-in)  
**Primary goal:** SDK-compatible endpoints, not a full cloud replica.

---

## Repository Layout

```
dashboard/                      # Vite + React + Tailwind dashboard (opt-in, port 8793)
  src/
    api/client.ts               # Typed fetch wrappers for /api/dashboard/* endpoints
    hooks/useApi.ts             # useApi / usePolling React hooks
    components/                 # layout/ and shared/ UI components
    pages/                      # One page per service (aws/, azure/, gcp/)
  vite.config.ts                # Dev server proxies /api/* → port 4793

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
        ses.py                  # AWS SES dataclasses (SesIdentity, SesMessage)
        s3.py                   # AWS S3 dataclasses (S3Bucket, S3Object)
        sns.py                  # AWS SNS dataclasses (SnsTopic, SnsSubscription, SnsMessage)
        sqs.py                  # AWS SQS dataclasses (SqsQueue, SqsMessage)
      azure/
        blob.py                 # Azure Blob dataclasses (AzureContainer, AzureBlob)
        servicebus.py           # Azure Service Bus dataclasses (AsbQueue, AsbTopic, AsbSubscription, AsbMessage)
      gcp/
        storage.py              # GCS dataclasses (GcsBucket, GcsObject)
        pubsub.py               # GCP Pub/Sub dataclasses (PubsubTopic, PubsubSubscription, PubsubMessage, PubsubAckable)
      common/
        events.py               # Shared dataclasses (Event)
    repositories/
      __init__.py               # make_repositories() factory + all re-exports
      aws/
        __init__.py             # Re-exports all AWS repo classes
        ses/
          repository.py         # Abstract: SesIdentityRepository, SesMessageRepository
          inmemory.py           # InMemory implementations
          sqlite.py             # SQLite implementations + DDL constant
          __init__.py
        s3/
          repository.py         # Abstract: S3BucketRepository, S3ObjectRepository
          inmemory.py
          sqlite.py             # + DDL
          __init__.py
        sns/
          repository.py         # Abstract: SnsTopicRepository, SnsSubscriptionRepository, SnsMessageRepository
          inmemory.py
          sqlite.py             # + DDL
          __init__.py
        sqs/
          repository.py         # Abstract: SqsQueueRepository, SqsMessageRepository
          inmemory.py
          sqlite.py             # + DDL
          __init__.py
      azure/
        __init__.py
        blob/
          repository.py         # Abstract: AzureContainerRepository, AzureBlobRepository
          inmemory.py
          sqlite.py             # + DDL
          __init__.py
        servicebus/
          repository.py         # Abstract: AsbQueueRepository, AsbTopicRepository, AsbSubscriptionRepository, AsbMessageRepository
          inmemory.py
          sqlite.py             # + DDL
          __init__.py
      gcp/
        __init__.py
        storage/
          repository.py         # Abstract: GcsBucketRepository, GcsObjectRepository
          inmemory.py
          sqlite.py             # + DDL
          __init__.py
        pubsub/
          repository.py         # Abstract: PubsubTopicRepository, PubsubSubscriptionRepository, PubsubMessageRepository, PubsubAckableRepository
          inmemory.py
          sqlite.py             # + DDL
          __init__.py
      common/
        __init__.py
        events/
          repository.py         # Abstract: EventRepository
          inmemory.py
          sqlite.py             # + DDL
          __init__.py

  providers/
    aws/
      provider.py               # AwsProvider – discovers + registers services,
                                #   owns the single POST / endpoint and routes:
                                #   application/x-amz-json-1.0 → json_router (SQS)
                                #   form-urlencoded (default)  → query_router (SES v1, SNS)
      protocols/
        query.py                # AWS Query dispatcher (SES v1, SNS – form-urlencoded)
        json_protocol.py        # AWS JSON dispatcher (SQS – X-Amz-Target header)
      ses/
        __init__.py             # PUBLIC ENTRY POINT: register(app, config, repos, telemetry, *, query_router, json_router)
        service.py              # SesService – pure business logic
        handlers.py             # v1 Query/XML HTTP handlers
        handlers_v2.py          # v2 REST/JSON HTTP handlers
        smtp.py                 # Optional SMTP relay adapter
      s3/
        __init__.py             # PUBLIC ENTRY POINT: register(app, config, repos, telemetry)
        service.py              # S3Service – pure business logic
        handlers.py             # REST HTTP handlers
      sns/
        __init__.py             # PUBLIC ENTRY POINT: register(app, config, repos, telemetry, *, query_router, json_router)
        service.py              # SnsService – pure business logic
        handlers.py             # Query/XML HTTP handlers (CreateTopic, ListTopics, Subscribe, Publish)
      sqs/
        __init__.py             # PUBLIC ENTRY POINT: register(app, config, repos, telemetry, *, query_router, json_router)
        service.py              # SqsService – pure business logic
        handlers.py             # JSON HTTP handlers (CreateQueue, SendMessage, ReceiveMessage, DeleteMessage)
    azure/
      provider.py               # AzureProvider – discovers + registers services
      blob/
        __init__.py             # PUBLIC ENTRY POINT: register(app, config, repos, telemetry)
        service.py              # AzureBlobService – pure business logic
        handlers.py             # REST HTTP handlers
      servicebus/
        __init__.py             # PUBLIC ENTRY POINT: register(app, config, repos, telemetry)
        service.py              # AsbService – pure business logic
        handlers.py             # REST/JSON HTTP handlers
    gcp/
      provider.py               # GcpProvider – discovers + registers services
      storage/
        __init__.py             # PUBLIC ENTRY POINT: register(app, config, repos, telemetry)
        service.py              # GcsService – pure business logic
        handlers.py             # REST HTTP handlers
      pubsub/
        __init__.py             # PUBLIC ENTRY POINT: register(app, config, repos, telemetry)
        service.py              # PubsubService – pure business logic
        handlers.py             # REST/JSON HTTP handlers

tests/
  integration/
    providers/
      aws/
        conftest.py             # Session-scoped uvicorn server + boto3 fixtures
        test_ses_boto3.py       # SES v1 integration tests (boto3)
        test_sesv2_boto3.py     # SES v2 integration tests (boto3)
        test_s3_boto3.py        # S3 integration tests (boto3)
        test_sns_boto3.py       # SNS integration tests (boto3)
        test_sqs_boto3.py       # SQS integration tests (boto3)
      azure/
        conftest.py             # Session-scoped uvicorn server + Azure SDK fixtures
        test_blob_sdk.py        # Azure Blob Storage tests
        test_servicebus_sdk.py  # Azure Service Bus tests
      gcp/
        conftest.py             # Session-scoped uvicorn server + GCP SDK fixtures
        test_storage_sdk.py     # GCP Cloud Storage tests
        test_pubsub_sdk.py      # GCP Pub/Sub tests
```

---

## Core Design Principles

1. **Single runtime, no external services.** No MinIO, no RabbitMQ, no mail
   servers. Everything runs in-process.

2. **SQLite as source of truth.** All services store state in SQLite. An
   in-memory mode (Python dicts) is also supported for CI and ephemeral testing.

3. **Services are pure Python.** `service.py` files contain only domain logic.
   They have no knowledge of HTTP, XML, or JSON. They depend only on repository
   interfaces and `TelemetryEngine`.

4. **Protocol adapter pattern.** HTTP/protocol concerns live in `handlers.py`
   (and `handlers_v2.py`). Handlers parse requests, call service methods, and
   format responses. They never contain business logic.

5. **Self-registering services.** Each service package exposes a single
   `register(app, config, repos, telemetry)` function via its `__init__.py`.
   `provider.py` discovers services by name and calls `register` — it contains
   no service-specific code.

6. **Repository abstraction.** Services depend on abstract repository base
   classes (e.g. `SesIdentityRepository`). Two concrete implementations exist:
   `SqliteXxxRepository` and `InMemoryXxxRepository`. Services never check which
   implementation is active.

---

## Key Patterns

### Adding a New AWS Service (e.g. Kinesis)

1. Create `src/cloudtwin/providers/aws/kinesis/` with:
   - `__init__.py` — must define `register(app, config, repos, telemetry, *, query_router, json_router) -> None`
   - `service.py` — pure business logic, depends on repos + telemetry
   - `handlers.py` — HTTP layer, registers actions into the appropriate shared router
2. Create `src/cloudtwin/persistence/repositories/aws/kinesis/` with:
   - `repository.py` — abstract interface(s)
   - `inmemory.py` — in-memory implementation(s)
   - `sqlite.py` — SQLite implementation(s) **plus a module-level `DDL` string constant**
   - `__init__.py` — re-exports all classes
3. Import the new `DDL` in `persistence/db.py` and add it to the `DDL` concatenation
4. Add any new model dataclasses to `persistence/models/aws/<service>.py`
5. Add the new repository key(s) to `make_repositories()` in `persistence/repositories/__init__.py`
6. Add `"kinesis"` to `_SERVICE_REGISTRY` in `providers/aws/provider.py`
7. Add `"kinesis"` to the services list in `config/cloudtwin.yml` and the default in `config.py`

For **Query-protocol** services (form-urlencoded like SNS): call `query_router.register("ActionName", handler)`.
For **JSON-protocol** services (like SQS): call `json_router.register("ServicePrefix.ActionName", handler)`.

`provider.py` itself does **not** need any other changes.

### Adding a New Cloud Provider (e.g. a new provider)

1. Create `src/cloudtwin/providers/<provider>/provider.py` with a `<Provider>Provider`
   class implementing `register(app)` using the same discovery pattern as
   `AwsProvider` — Azure and GCP are existing examples to follow
2. Add its service packages under `providers/<provider>/`
3. Add its repository packages under `persistence/repositories/<provider>/`
4. Wire it in `app.py` alongside `AwsProvider`, `AzureProvider`, and `GcpProvider`

### Service Pattern

```python
# service.py – no HTTP, no XML, no JSON
class FooService:
    def __init__(self, config, repo: FooRepository, telemetry: TelemetryEngine):
        ...

    async def do_thing(self, ...) -> ...:
        # validate, mutate state via repo, emit telemetry
        await self._repo.save(...)
        await self._telemetry.emit("aws", "foo", "do_thing", {...})
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

### Repository Pattern

Every new entity needs:
- A dataclass in `persistence/models/<provider>/<service>.py`
- An abstract class in `persistence/repositories/<provider>/<service>/repository.py` (inherits `ABC`)
- A `sqlite.py` with the SQLite implementation **and** a `DDL` module-level string constant
- An `inmemory.py` with the in-memory implementation
- An `__init__.py` re-exporting all classes
- The `DDL` imported and concatenated in `persistence/db.py`
- An entry in the `make_repositories()` factory in `persistence/repositories/__init__.py`

### AWS Protocol Reference

| Service | Protocol | Content-Type | Dispatch mechanism | Endpoint |
|---|---|---|---|---|
| SES v1 | query | `application/x-www-form-urlencoded` | `Action` form field → `QueryProtocolRouter` | `POST /` |
| SNS | query | `application/x-www-form-urlencoded` | `Action` form field → `QueryProtocolRouter` | `POST /` |
| SQS | json | `application/x-amz-json-1.0` | `X-Amz-Target` header → `JsonProtocolRouter` | `POST /` |
| SES v2 | REST/JSON | `application/json` | FastAPI path routing | `POST /v2/...` |
| S3 | REST | varies | FastAPI path routing | `PUT/GET/DELETE /{bucket}/...` |

`AwsProvider` owns the single `POST /` endpoint and routes by `Content-Type`:
- `application/x-amz-json-1.0` → `json_router` (SQS)
- anything else → `query_router` (SES v1, SNS)

Both routers are created by `AwsProvider` and passed to each service's `register()` via keyword args (`query_router=`, `json_router=`). S3 is exempt — it mounts its own REST routes.

### SES v1 vs v2 Protocol Split (legacy table)

| | SES v1 | SES v2 |
|---|---|---|
| Protocol | AWS Query (form-urlencoded) | REST/JSON |
| Request format | `Action=SendEmail&Source=...` | `POST /v2/email/outbound-emails` JSON body |
| Response format | XML with AWS namespace | JSON |
| Mounted at | `POST /` | `POST /v2/...` |
| Handlers | `ses/handlers.py` | `ses/handlers_v2.py` |

Both share a single `SesService` instance — identity and message state is unified
across protocol versions.

### Error Handling

Raise subclasses of `CloudTwinError` from service code:

```python
from cloudtwin.core.errors import NotFoundError, ValidationError, IdentityNotVerifiedError

raise NotFoundError("Bucket not found: my-bucket")   # → HTTP 404
raise ValidationError("Invalid key format")           # → HTTP 400
raise IdentityNotVerifiedError("sender@example.com")  # → HTTP 400, code MessageRejected
```

Handlers catch `CloudTwinError` and format it appropriately for their protocol
(XML for SES v1, JSON for SES v2 and S3).

### Configuration

Config is loaded via `load_config()` which merges (highest priority first):
1. Environment variables (`CLOUDTWIN_*`)
2. YAML file at `CLOUDTWIN_CONFIG_PATH` (default `/config/cloudtwin.yml`)
3. Built-in defaults

Access config in service packages via the `Config` dataclass passed to `register()`.
Never read environment variables directly inside service or handler code.

### Storage Modes

- `CLOUDTWIN_STORAGE_MODE=sqlite` (default) — persists to `/data/cloudtwin.db`
- `CLOUDTWIN_STORAGE_MODE=memory` — pure in-memory, no file I/O, used in tests

Services and handlers never check the storage mode — the `make_repositories()`
factory returns the correct implementation transparently.

### Telemetry

Every service action should emit an event:

```python
await self._telemetry.emit("aws", "ses", "send_email", {"source": source, ...})
```

Events are written to the `events` table and will be consumed by the future
dashboard. The format is `provider.service.action`.

---

## Testing

### Integration Tests (preferred for new services)

Integration tests use a real uvicorn server (in-memory mode) and the official
boto3/SDK clients — no mocks. Located at
`tests/integration/providers/<provider>/<service>/`.

```python
# conftest.py provides: server_url, ses, sesv2, s3 fixtures (session-scoped)
def test_something(s3):
    s3.create_bucket(Bucket="my-bucket")
    resp = s3.get_object(Bucket="my-bucket", Key="file.txt")
    assert resp["Body"].read() == b"hello"
```

Run with: `make test-integration`

When adding a new service, add a new fixture to the appropriate `conftest.py` under `tests/integration/providers/`:

```python
@pytest.fixture(scope="session")
def kinesis(server_url):
    import boto3
    return boto3.client("kinesis", endpoint_url=server_url, **_FAKE_CREDS)
```

Also add the new service name to the `services=[...]` list in the `server_url` fixture.

### Storage in Tests

Always use `StorageConfig(mode="memory")` — never create SQLite files in tests.
The session-scoped server shares state across all tests in a session; use unique
resource names (bucket names, domains) per test to avoid cross-test interference.

---

## Conventions

- `from __future__ import annotations` at the top of every file
- Async throughout — all repository methods and service methods are `async`
- Dataclasses for all models (`persistence/models.py`) — no ORM, no Pydantic in
  the persistence layer
- `__init__.py` files are used only for re-exports and the `register()` entry
  point — never for instantiation or side effects
- Logger names follow the module hierarchy: `logging.getLogger("cloudtwin.ses")`,
  `logging.getLogger("cloudtwin.s3")`, etc.
- ISO 8601 UTC strings for all timestamps (`datetime.now(timezone.utc).isoformat()`)

---

## Documentation Maintenance

Whenever you add, modify, or remove a service, provider, or architectural pattern, you **must** also update the following documents:

### `README.md`
- Add the new service/provider to the supported services table or feature list
- Update any quickstart examples if the new service changes how CloudTwin is invoked
- Update port/endpoint references if new routes are introduced
- Remove or correct any outdated capability descriptions

### `docs/developer-guide.md`
- Add a new section (or subsection) describing the new service: its purpose, supported operations, and any SDK compatibility notes
- Document any non-obvious configuration options introduced by the service
- Add example SDK usage snippets for the new service (boto3, Azure SDK, or GCP SDK as appropriate)
- If a new protocol or dispatch mechanism was introduced, document it in the protocol reference section

### `.github/copilot-instructions.md` *(this file)*
- If a **new design or architectural principle** is introduced (e.g. a new dispatch pattern, a new layer, a new abstraction), add it to the **Core Design Principles** or **Key Patterns** section
- If an existing principle is changed or clarified, update the relevant section in place
- If a new cloud provider is added, update the repository layout tree and the "Adding a New Cloud Provider" section
- If a new AWS (or Azure/GCP) service pattern diverges from existing conventions, document the divergence explicitly so future contributors follow the correct pattern

### When These Updates Are Required

| Trigger | `README.md` | `docs/developer-guide.md` | `copilot-instructions.md` |
|---|---|---|---|
| New service added | ✅ | ✅ | Only if new pattern introduced |
| New cloud provider added | ✅ | ✅ | ✅ |
| New protocol/dispatch mechanism | ✅ | ✅ | ✅ |
| Architectural principle changed | ✅ (if user-facing) | ✅ | ✅ |
| Bug fix / minor change | ❌ | ❌ | ❌ |

> **Rule:** If you would tell a new contributor "here's how this works" — it belongs in the docs. Never leave architecture undocumented.
