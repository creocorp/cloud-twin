# Changelog

All notable changes to CloudTwin. Versions follow [Semantic Versioning](https://semver.org/);
release images are published to Docker Hub as `creogroup/cloudtwin:<version>` (plus
`<major>.<minor>` and `latest`) for `linux/amd64` and `linux/arm64`.

---

## [0.4.1] — 2026-10-30

### Added

- **Cognito: CORS for browser apps.** OIDC discovery
  (`/{poolId}/.well-known/openid-configuration`), JWKS
  (`/{poolId}/.well-known/jwks.json`), `/oauth2/token`, `/oauth2/userInfo`, and
  `/oauth2/revoke` now answer CORS preflight (`OPTIONS`) requests and return
  `Access-Control-Allow-Origin` (the request origin, with
  `Access-Control-Allow-Credentials: true`) on success *and* error responses.
  A single-page app can now run the Authorization Code + PKCE flow entirely in
  the browser — discovery, code exchange, userInfo, and revoke — without a
  backend token exchange or a dev-server proxy. Previously these responses had no
  CORS headers and preflights returned `405`, so browsers blocked them (including
  the discovery fetch most OIDC libraries make first).
- **`providers.aws.cognito.cors_allowed_origins`** /
  **`CLOUDTWIN_COGNITO_CORS_ALLOWED_ORIGINS`** (comma-separated) restricts which
  origins may make those calls. Empty (the default) allows any origin.

The hosted-UI pages (`/oauth2/authorize`, `/login`, `/logout`) are top-level
navigations and intentionally send no CORS headers.

### Upgrading from 0.4.0

No configuration changes are required.

---

## [0.4.0] — 2026-10-29

CloudTwin is now a single Rust binary. The former **CloudTwin Lite** runtime
becomes *the* CloudTwin, the Python backend is retired, and the AWS emulation
gains the services and behaviours needed to run a real mail, storage, and
sign-in stack locally.

### ⚠️ Breaking changes

- **The Python backend is removed.** The FastAPI server, the `cloudtwin` /
  `cloudtwin-cli` Python commands, and `pyproject.toml` are gone. The source is
  preserved on the [`archive/python`](https://github.com/creocorp/cloud-twin/tree/archive/python)
  branch.
- **Services only the Python backend had are not available yet:** AWS Lambda,
  Azure Queue Storage / Event Grid / Key Vault / Functions, and GCP Firestore /
  Cloud Tasks / Secret Manager / Cloud Functions. Their SDK test suites are kept
  (skipped) as the spec for porting them.
- **Python-era database files are not compatible.** CloudTwin refuses to start on
  them with a clear error; point `CLOUDTWIN_DB_PATH` at a new file or use a fresh
  `/data` volume.
- **Image tags:** `creogroup/cloudtwin:lite-*` tags are no longer published. Use
  the regular tags (`0.4.0`, `0.4`, `latest`).
- **Defaults and names:** the binary is `cloudtwin` (was `cloudtwin-lite`) and the
  default database is `/data/cloudtwin.db` (was `/data/cloudtwin-lite.db`). The
  Python-only settings `CLOUDTWIN_API_PORT`, `CLOUDTWIN_STORAGE_MODE`,
  `CLOUDTWIN_STORAGE_PATH`, and `CLOUDTWIN_DASHBOARD_ENABLED` are replaced by
  `CLOUDTWIN_PORT`, `CLOUDTWIN_DB_PATH` (`:memory:` for in-memory), and
  `CLOUDTWIN_DASHBOARD_STATIC`.
- **SES sender verification is on by default** (`strict_verification: true`):
  sends from unverified identities are rejected with `MessageRejected`, as in AWS.
  Set `CLOUDTWIN_SES_STRICT_VERIFICATION=false` for the old behaviour.
- **AWS-accurate errors replace lenient success** in a few places: creating an
  existing SES identity raises `AlreadyExistsException`, deleting a missing one
  raises `NotFoundException`, creating an existing secret raises
  `ResourceExistsException`, and creating an existing GCS bucket or Pub/Sub
  topic/subscription returns `409`. Deleting a non-empty GCS bucket is refused.
- **Azure Service Bus HTTP responses** use the original CloudTwin shape
  (`{"queues": [...]}`, `{"messages": [{"message_id", "lock_token", "body", ...}]}`)
  rather than the `{"value": [...]}` shape the Lite image briefly used.

### Added — AWS SES

- **DKIM and verification status:** `CreateEmailIdentity` / `GetEmailIdentity`
  return `DkimAttributes` (three generated `Tokens`, `Status`,
  `SigningAttributesOrigin: AWS_SES`), `VerificationStatus`, and
  `VerificationInfo`. v1 `VerifyDomainDkim`, `GetIdentityDkimAttributes`, and
  `SetIdentityDkimEnabled`.
- **Controllable verification** (`providers.aws.ses.verification`): `auto`
  (default), `manual` (stays `PENDING` until forced), `after_checks` (resolves
  after N reads), or `delay` (after N seconds); final outcome `SUCCESS`, `FAILED`,
  or `TEMPORARY_FAILURE`; per-identity `overrides` with `*.domain` / `*@domain`
  globs. Pending email identities receive a verification email in the inbox whose
  link verifies them.
- **AWS-shaped REST errors:** SES v2 errors carry `x-amzn-ErrorType` and `__type`
  so boto3 and the AWS Rust SDK raise typed exceptions (`NotFoundException`,
  `AlreadyExistsException`, `MessageRejected`, ...).
- **Custom MAIL FROM:** `PutEmailIdentityMailFromAttributes` and
  `MailFromAttributes` (`MailFromDomain`, `MailFromDomainStatus`,
  `BehaviorOnMxFailure`); v1 `SetIdentityMailFromDomain` /
  `GetIdentityMailFromDomainAttributes`. A pending MAIL FROM with
  `REJECT_MESSAGE` blocks sends (`MailFromDomainNotVerifiedException`).
- **Account:** `GetAccount` (`SendQuota` with real `SentLast24Hours`,
  `ProductionAccessEnabled`, `SendingEnabled`, `EnforcementStatus`),
  `PutAccountSendingAttributes`, `PutAccountSuppressionAttributes`; v1
  `GetSendQuota`, `GetAccountSendingEnabled`, `UpdateAccountSendingEnabled`.
  Sandbox mode (`production_access_enabled: false`) requires verified recipients.
- **Raw and richer sends:** v2 `Content.Raw` and v1 `SendRawEmail` (full MIME with
  attachments and threading headers), `ReplyToAddresses`, `EmailTags`,
  `ConfigurationSetName`, `FeedbackForwardingEmailAddress`, and
  `Content.Simple.Headers`.
- **Configuration sets and events:** v2 and v1 configuration-set APIs with event
  destinations; `Send`, `Delivery`, `Bounce`, `Complaint`, `Reject`, and
  `DeliveryDelay` events in SES's JSON shape are published to SNS destinations.
- **Mailbox simulator:** `success@`, `bounce@`, `complaint@`, `suppressionlist@`,
  and `ooto@simulator.amazonses.com` (plus CloudTwin's `delay@` and `reject@`),
  including bounce/complaint feedback notifications.
- **Account suppression list:** `Put`/`Get`/`List`/`DeleteSuppressedDestination`;
  sends to listed addresses bounce with `OnAccountSuppressionList`.
- **Email receiving:** v1 receipt rule sets and rules (`Create`/`Delete`/`List`/
  `Describe`/`Clone`/`Reorder`, `Set`/`DescribeActiveReceiptRuleSet`, rule CRUD and
  positioning). Mail arriving on the SMTP listener (port `1025`) for recipients
  matched by the active rule set runs `S3Action` (writes the raw message, 40 MB
  limit), `SNSAction`, `AddHeaderAction`, `BounceAction`, and `StopAction`;
  unmatched recipients still go to the capture inbox.

### Added — AWS S3

- `ListObjectsV2` paging (`continuation-token`, `start-after`, real
  `IsTruncated`), `delimiter` / `CommonPrefixes`, `encoding-type=url`;
  ListObjectVersions and ListMultipartUploads.
- User metadata (`x-amz-meta-*`) and standard headers, object and bucket tagging,
  `CopyObject`, ranged and conditional `GET`, `DeleteObjects`.
- Versioning (version ids, delete markers, `?versionId=`), SSE headers
  (`AES256`, `aws:kms` with key id) and default bucket encryption, Block Public
  Access, lifecycle, policy, ownership controls, and other bucket settings
  (stored and echoed; lifecycle is not enforced).
- Multipart upload (`Create`/`UploadPart`/`UploadPartCopy`/`Complete`/`Abort`/
  `ListParts`).
- Bucket CORS (`PutBucketCors` + `OPTIONS` preflight) and presigned `POST`
  policies; virtual-hosted addressing (`{bucket}.s3.localhost`).
- Event notifications (`Put`/`GetBucketNotificationConfiguration`) to SQS and SNS
  with prefix/suffix filters and AWS-shaped event JSON.
- `aws-chunked` streaming uploads with checksum trailers and trailing-slash
  bucket paths, so the AWS Rust SDK works alongside boto3.

### Added — AWS SQS and SNS

- **SQS:** long polling (`WaitTimeSeconds`), per-queue and per-message
  visibility timeouts, `DelaySeconds`, dead-letter queues via `RedrivePolicy` /
  `maxReceiveCount`, message attributes with AWS-exact `MD5OfMessageAttributes`,
  system attributes (`ApproximateReceiveCount`, `SentTimestamp`, ...), batch send /
  delete / change-visibility, `SetQueueAttributes`, `PurgeQueue`, tags, and FIFO
  queues. Errors are typed (`QueueDoesNotExist`, ...).
- **SNS:** `Publish` now delivers — to SQS (notification envelope, or the raw
  message with `RawMessageDelivery`), HTTP(S) endpoints, and email; filter
  policies, `PublishBatch`, `MessageStructure=json`, subscription and topic
  attributes, `Unsubscribe`, `ListSubscriptions`, and tags. Subscriptions are
  auto-confirmed.

### Added — new AWS services

- **KMS:** symmetric keys with real AES-256-GCM, aliases, `Encrypt` / `Decrypt` /
  `ReEncrypt`, data keys, encryption context, enable/disable, scheduled deletion,
  rotation flags, tags, and key policies.
- **Cognito user pools:** OIDC discovery and JWKS with a stable RS256 key, a hosted
  login page with Authorization Code + PKCE, `/oauth2/token`
  (`authorization_code`, `refresh_token`, `client_credentials`), `userInfo`,
  `revoke`, and `logout`; Cognito-shaped ID and access tokens; and the admin and
  user API (`AdminCreateUser` with invite email, `ListUsers` filters,
  enable/disable/delete, passwords, global sign-out, `InitiateAuth` with
  `NEW_PASSWORD_REQUIRED`, sign-up and forgot-password).
- **DynamoDB `Query`** with `KeyConditionExpression` (partition-key equality plus
  `=`, `<`, `<=`, `>`, `>=`, `BETWEEN`, `begins_with` on the sort key).

### Added — Azure and GCP SDK compatibility

- **Azure Blob Storage** works with the official `azure-storage-blob` SDK:
  container list/get/create/delete, single-shot uploads with overwrite
  protection, ranged downloads, prefix/delimiter listing with paging, and the
  headers the SDK expects. (Block uploads above 64 MiB are not supported.)
- **Azure Service Bus** HTTP API: queues, topics, subscriptions, send / receive /
  complete / abandon / dead-letter, `?limit=`, and fan-out to every subscription.
- **GCP Cloud Storage** works with `google-cloud-storage`: media, multipart, and
  resumable uploads, `alt=media` downloads with `Range`, full object resources
  (`generation`, `md5Hash`, `crc32c`, links), delimiter listing, metadata patch,
  and `ifGenerationMatch`.
- **GCP Pub/Sub** works with `google-cloud-pubsub` (REST transport): topic and
  subscription CRUD, publish, pull with ack-deadline leases, acknowledge, and
  `modifyAckDeadline`.
- Azure and GCP are served at the **root paths SDKs use by default**
  (`http://localhost:4793/devstoreaccount1`, `/{namespace}/queues`,
  `/storage/v1`, `/v1/projects`) as well as under `/azure` and `/gcp`.

### Added — local development and testing

- **Test control plane:** `POST /_cloudtwin/reset` (all or selected services,
  re-applies the startup seed) and `POST /_cloudtwin/seed`; SES controls under
  `/_cloudtwin/ses` (force verification, account overrides, event log, sent
  messages, and `POST /_cloudtwin/ses/inbound` to deliver mail without SMTP).
- **Seeding:** a `seed:` block in `cloudtwin.yml` or `CLOUDTWIN_SEED_PATH`
  provisions buckets, queues, topics and subscriptions, SES identities /
  configuration sets / receipt rules / suppression entries, KMS keys, and
  Cognito pools, clients, and users at startup — idempotently.
- **Configuration:** per-service settings under `providers.aws.{ses,s3,cognito}`
  with `CLOUDTWIN_*` overrides; `dashboard.static_path`; `CLOUDTWIN_BASE_URL`.
  See the [developer guide](docs/developer-guide.md#configuration).
- Database files from earlier Rust releases upgrade in place (additive schema
  migrations).

### Changed — project

- The crate lives at the repository root; one `docker/Dockerfile` and one publish
  workflow (`v*` tags).
- The Python SDK integration suites (boto3, Azure, GCP) remain the compatibility
  spec and now run against the binary (`make test-integration`), alongside
  in-process Rust tests (`make test-unit`). CI runs `cargo fmt`, clippy
  (`-D warnings`), `cargo test`, and the SDK suites.
- README, developer guide, and contributor instructions rewritten for the Rust
  runtime.

### Upgrading from 0.3.x

1. Switch the image to `creogroup/cloudtwin:0.4` (from `latest` on the Python
   image or from `lite-*`).
2. Start from a fresh `/data` volume if you used the Python image; Lite volumes
   keep working, but rename `cloudtwin-lite.db` to `cloudtwin.db` or set
   `CLOUDTWIN_DB_PATH=/data/cloudtwin-lite.db`.
3. Replace Python-only environment variables (see *Breaking changes*).
4. Verify SES senders (or seed them under `seed: ses: identities:`), or set
   `CLOUDTWIN_SES_STRICT_VERIFICATION=false`.
5. If you mount your own `cloudtwin.yml`, check
   `providers.azure.servicebus.namespace` (default `cloudtwin`) matches what your
   clients use.

[0.4.1]: https://github.com/creocorp/cloud-twin/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/creocorp/cloud-twin/compare/v0.3.1...v0.4.0
