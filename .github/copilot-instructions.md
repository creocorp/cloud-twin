# GitHub Copilot Instructions

## Project Overview

CloudTwin is a lightweight, self-contained multi-cloud local runtime: a single
Rust binary (axum + tokio + SQLite) that emulates AWS (SES, S3, SNS, SQS,
DynamoDB, Secrets Manager, KMS, Cognito, Bedrock), Azure (Blob, Service Bus),
and GCP (Cloud Storage, Pub/Sub) APIs with no external dependencies. State is
persisted in SQLite or kept in memory (`:memory:`). It is a drop-in endpoint
override for SDK-based application code during local development and CI.

**Ports:** `4793` → cloud APIs + dashboard · `1025` → SMTP (mailbox + SES receiving)
**Primary goal:** SDK-compatible endpoints, not a full cloud replica.

The Python backend was retired in 0.4; its source lives on the `archive/python`
branch and can be read as a reference when porting the services it had that
Rust lacks (Lambda, Azure Queue/Event Grid/Key Vault/Functions, GCP
Firestore/Cloud Tasks/Secret Manager/Cloud Functions).

---

## Repository Layout

```
Cargo.toml / Cargo.lock
config/cloudtwin.yml        # Default config shipped in the Docker image
dashboard/static/           # Dashboard UI (plain JS), served at /dashboard
docker/Dockerfile           # Release image (creogroup/cloudtwin)
docs/developer-guide.md     # Architecture, configuration, per-service reference
scripts/demo.py             # Seeds a running server with demo data
src/
  main.rs  app.rs  config.rs  db.rs  proto.rs  xml.rs
  control.rs  seed.rs  telemetry.rs  dashboard.rs  testutil.rs
  mailbox/                  # SMTP listener + capture inbox
  aws/<service>/            # ses s3 sns sqs kms cognito dynamodb secretsmanager bedrock
  azure/<service>/          # blob servicebus
  gcp/<service>/            # storage pubsub
tests/                      # Python SDK integration suites (pytest + boto3/Azure/GCP SDKs)
  conftest.py               # Spawns target/debug/cloudtwin with tests/cloudtwin-test.yml
  integration/providers/{aws,azure,gcp}/  integration/dashboard/
```

---

## Architecture

Layout and patterns for `src/` (details in `docs/developer-guide.md`):

- `app.rs` — `AppState { db, cfg, bedrock, http }`, `build_router()`, and the AWS
  `POST /` dispatcher (JSON protocol by `X-Amz-Target` prefix: `AmazonSQS.`,
  `DynamoDB_`, `secretsmanager.`, `TrentService.`, `AWSCognitoIdentityProviderService.`;
  Query protocol by `Action`: SES v1, then SNS). It also rewrites
  virtual-hosted S3 hosts (`{bucket}.s3.localhost`) before routing.
- `aws/<service>/` — `mod.rs` (public surface), `handlers.rs` (protocol only),
  `service.rs` (logic only), `models.rs`, `schema.rs`, `tests.rs`, optional
  `config.rs` (`providers.aws.<service>` YAML + `CLOUDTWIN_*` env via `apply_env`).
- **Schema ownership:** each module's `schema.rs::migrate(conn)` creates its own
  tables (`CREATE TABLE IF NOT EXISTS`) and adds later columns with
  `db::ensure_column` so existing database files upgrade in place. Table names
  must start with the module prefix (`ses_`, `s3_`, `sqs_`, `sns_`, `kms_`,
  `cognito_`, ...) so `/_cloudtwin/reset` can clear them. State that must
  survive resets (e.g. signing keys) goes in `cloudtwin_meta` via `db::meta_get/meta_set`.
- **Cross-service calls** use internal functions, never HTTP:
  `aws::sqs::send_internal`, `aws::sns::publish_internal`,
  `aws::s3::put_object_internal`, `mailbox::service::capture_raw`. Keep their
  signatures stable; other modules depend on them.
- **Seeding:** each module exposes `pub async fn seed(state, doc)` that
  provisions its section of a seed document idempotently. `seed.rs` applies
  sections in dependency order at startup (`seed:` YAML / `CLOUDTWIN_SEED_PATH`),
  on `POST /_cloudtwin/seed`, and after `POST /_cloudtwin/reset`.
- **Test control plane:** CloudTwin-only endpoints live under `/_cloudtwin/*`
  (`control.rs`, plus per-module routers such as `aws::ses::control_router()`).
  Never put test hooks on AWS API paths.
- **Wire fidelity:** match AWS exactly — error codes and headers
  (`proto::rest_json_error` sets `x-amzn-ErrorType`; JSON errors use `__type`;
  Query errors are XML), timestamp formats (restJson/awsJson epoch seconds,
  S3 XML `%Y-%m-%dT%H:%M:%S%.3fZ`, S3 headers RFC 7231), and request shapes from
  both boto3 and the AWS Rust SDK (e.g. trailing-slash bucket paths,
  `aws-chunked` uploads). Prefer AWS behaviour over convenience (e.g.
  `AlreadyExistsException` on duplicate creates).
- **Tests:** in-process tests use `crate::testutil::TestApp` (real router,
  in-memory DB) and drive the app over HTTP like an SDK. The boto3 suite can
  target a running binary via `CLOUDTWIN_TEST_URL`. Run `make test-unit`,
  `make test-integration`, and `make lint` (`cargo fmt --check`, clippy `-D warnings`).

To add a service: create the module folder, call its `schema::migrate`
from `Database::migrate`, wire its protocol in `app.rs`, add its `seed` to
`seed.rs` and its table prefix to `SERVICE_TABLES` in `control.rs`.

---

## Testing

- **Rust in-process tests** (`src/**/tests.rs`, `cargo test`) use
  `crate::testutil::TestApp`. Add them for new behaviour.
- **Python SDK suites** (`tests/`, `make test-integration`) are the
  compatibility spec: they drive the binary with boto3, the Azure SDK, and the
  GCP SDKs. Keep them; add SDK-level tests for new services or wire behaviour.
  Expectations must match real cloud behaviour (e.g. duplicate creates raise).
  Suites for services not yet ported are marked `pytestmark = pytest.mark.skip(...)`.
- The SDK server is shared per session — use unique resource names (or
  `POST /_cloudtwin/reset`). Never create SQLite files in tests; the fixture
  uses `:memory:` unless `CLOUDTWIN_STORAGE_MODE=sqlite`.

---

## Conventions

- Async throughout; DB work goes through `state.db.call(...)`.
- Never read environment variables in handlers/services — use `state.cfg`
  (module `config.rs` + `apply_env`).
- Telemetry: call `crate::telemetry::emit(&db, provider, service, action, payload)`
  after successful mutations.
- Doc comments only where they add information; keep `cargo fmt` and
  `cargo clippy --all-targets -- -D warnings` clean.

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
- If a **new design or architectural principle** is introduced (e.g. a new dispatch pattern, a new layer, a new abstraction), add it to the **Architecture** section
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
