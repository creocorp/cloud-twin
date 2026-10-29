# CloudTwin — Developer Guide

This guide is for contributors and anyone wanting to understand, extend, or
modify CloudTwin. CloudTwin is a single Rust binary (axum + tokio + SQLite)
that emulates AWS, Azure, and GCP APIs for local development and CI.

---

## Contents

- [Architecture](#architecture)
- [Directory layout](#directory-layout)
- [Configuration](#configuration)
- [Test control plane](#test-control-plane-_cloudtwin)
- [Supported operations](#supported-operations)
- [AWS protocol reference](#aws-protocol-reference)
- [AWS Bedrock simulation](#aws-bedrock-simulation)
- [Testing](#testing)
- [Adding a new service](#adding-a-new-service)
- [Dashboard](#dashboard)
- [Not yet implemented](#not-yet-implemented)

---

## Architecture

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

**Design principles**

1. **Single runtime, no external services.** No MinIO, no RabbitMQ, no mail
   servers. Everything runs in-process.
2. **SQLite as source of truth.** A file for persistence, `:memory:` for CI.
   Schema changes are additive (`ensure_column`) so database files upgrade in place.
3. **Thin protocol adapters.** Handlers translate wire formats; services hold
   behaviour; neither reads environment variables.
4. **Wire fidelity over convenience.** Match AWS/Azure/GCP error codes, headers,
   timestamp formats, and request shapes from the official SDKs (boto3 *and*
   the AWS Rust SDK), even when a looser behaviour would be easier.

---

## Directory layout

```
Cargo.toml
config/cloudtwin.yml   # Default config shipped in the Docker image
dashboard/static/      # Dashboard UI (plain JS, no build step)
docker/Dockerfile
tests/                 # Python SDK integration suites (boto3, Azure, GCP)
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

---

## Configuration

CloudTwin reads `cloudtwin.yml` at `CLOUDTWIN_CONFIG_PATH` (default
`/config/cloudtwin.yml`; the document may be wrapped in a `cloudtwin:` key).
Environment variables win over YAML. Module settings live in each module's
`config.rs` with an `apply_env` for its `CLOUDTWIN_*` overrides; handlers and
services read `state.cfg`, never the environment.

| YAML | Env | Default | Purpose |
|---|---|---|---|
| `api_port` | `CLOUDTWIN_PORT` | `4793` | HTTP port |
| `storage.path` | `CLOUDTWIN_DB_PATH` | `/data/cloudtwin.db` | SQLite file (`:memory:` for ephemeral) |
| — | `CLOUDTWIN_BASE_URL` | `http://localhost:{port}` | Public URL used in queue URLs, links, OIDC issuer |
| `dashboard.static_path` | `CLOUDTWIN_DASHBOARD_STATIC` | — | Dashboard UI directory served at `/dashboard` |
| `providers.azure.blob.account_name` | `CLOUDTWIN_AZURE_ACCOUNT` | `devstoreaccount1` | Azure Blob account (also a root-path alias) |
| `providers.azure.servicebus.namespace` | `CLOUDTWIN_AZURE_NAMESPACE` | `cloudtwin` | Service Bus namespace (also a root-path alias) |
| `providers.gcp.project` | `CLOUDTWIN_GCP_PROJECT` | `cloudtwin` | Default GCP project |
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

---

## Test control plane (`/_cloudtwin`)

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

---

## Supported operations

### AWS SES (v1 Query + v2 REST)

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

### Mailbox — SMTP listener (`smtp://:1025`)

A built-in SMTP sink that captures mail for local inspection, implemented in
`src/mailbox/`. `smtp.rs` is a minimal hand-rolled SMTP
state machine (EHLO/HELO, MAIL, RCPT, DATA, RSET, NOOP, QUIT; no TLS/auth, no
relay). `service.rs::capture_raw` is the single ingestion path: it parses the
RFC 822 message with `mail-parser` and writes to `mailbox_messages` /
`mailbox_attachments`. SES v1/v2 `SendEmail` and `SendRawEmail` also capture
through it, so sent mail shows up in the same inbox. Before capture, each
message runs through the active SES receipt rule set; recipients matched by an
enabled rule are handled by its actions instead (see
[AWS SES](#aws-ses-v1-query--v2-rest)).

| Setting | YAML | Env | Default |
|---|---|---|---|
| Enable | `mailbox.enabled` | `CLOUDTWIN_MAILBOX_ENABLED` | `false` (shipped config: `true`) |
| Port | `mailbox.smtp_port` | `CLOUDTWIN_SMTP_PORT` | `1025` |

| Endpoint | Description |
|---|---|
| `GET /api/dashboard/mailbox` | List captured messages (newest first, max 200) |
| `GET /api/dashboard/mailbox/{id}` | Full message detail including attachment metadata |

### AWS S3 (REST, `/{bucket}/{key}`)

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

### AWS SQS (JSON) and SNS (Query)

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

### AWS KMS (JSON, `TrentService.*`)

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

### AWS Cognito user pools

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

### AWS DynamoDB (JSON, `DynamoDB_20120810.*`)

CreateTable, DescribeTable, ListTables, DeleteTable, PutItem, GetItem,
DeleteItem, UpdateItem (`SET` expressions), Scan, Query
(`KeyConditionExpression` with partition-key equality plus an optional
sort-key condition — `=`, `<`, `<=`, `>`, `>=`, `BETWEEN`, `begins_with` —
with `ScanIndexForward`, `Limit`, `Select=COUNT`), BatchWriteItem, BatchGetItem.

### AWS Secrets Manager (JSON 1.1, `secretsmanager.*`)

CreateSecret (duplicate names raise `ResourceExistsException`), GetSecretValue,
PutSecretValue, DescribeSecret, ListSecrets, DeleteSecret.

### AWS Bedrock (simulation, REST)

Bedrock is a simulation engine — it does not call real models. All responses are
generated deterministically from configuration.

| Method + Path | SDK operation | Description |
|---|---|---|
| `GET /foundation-models` | `bedrock.list_foundation_models()` | List known model stubs |
| `POST /model/{modelId}/invoke` | `bedrock-runtime.invoke_model()` | Invoke a model (non-streaming) |
| `POST /model/{modelId}/invoke-with-response-stream` | `bedrock-runtime.invoke_model_with_response_stream()` | Invoke a model with EventStream streaming |

Static routes such as `/foundation-models` always win over S3's `/{bucket}`
parameter routes, so a bucket with one of these names is shadowed.

### Azure Blob Storage (REST, `/{account}/...`)

Served at `/azure/{account}/...` and, for SDK defaults, at the root
(`account_url = http://localhost:4793/devstoreaccount1`). Shared-key and SAS
signatures are accepted without validation.

- Containers: create (409 `ContainerAlreadyExists`), get properties, delete
  (removes its blobs), list (`?comp=list` with `prefix`, `maxresults`, `marker`).
- Blobs: single-shot `Put Blob` (`x-ms-blob-type: BlockBlob`, `If-None-Match: *`
  → 409 for `overwrite=False`), get (including `Range` / `x-ms-range` → 206),
  get properties, delete, list (`restype=container&comp=list` with `prefix`,
  `delimiter` → `BlobPrefix`, paging).
- Responses carry `x-ms-version`, `x-ms-request-id`, `ETag`, `Last-Modified`
  (RFC 1123), `Content-MD5`, and `x-ms-error-code` on errors.
- Not supported: block uploads (`Put Block` / `Put Block List`, used by the
  SDKs above 64 MiB) — they return 400.

```python
from azure.core.credentials import AzureNamedKeyCredential
from azure.storage.blob import BlobServiceClient
blob = BlobServiceClient("http://localhost:4793/devstoreaccount1",
                         credential=AzureNamedKeyCredential("devstoreaccount1", "key"))
blob.create_container("uploads").upload_blob("a.txt", b"hello")
```

### Azure Service Bus (REST/JSON, `/{namespace}/...`)

HTTP API (the Service Bus SDK speaks AMQP, which CloudTwin does not emulate).
Served at `/azure/{namespace}/...` and at the root.

| Method + path | Description |
|---|---|
| `PUT` / `GET` / `DELETE /{ns}/queues/{queue}` | Create, get, delete a queue |
| `GET /{ns}/queues` | List queues |
| `POST /{ns}/queues/{queue}/messages` | Send (`{"body": ...}`) |
| `GET /{ns}/queues/{queue}/messages?limit=N` | Receive and lock up to N messages |
| `DELETE /{ns}/queues/{queue}/messages/{lock_token}` | Complete |
| `POST /{ns}/queues/{queue}/messages/{lock_token}/abandon` | Abandon (unlock) |
| `POST /{ns}/queues/{queue}/messages/{lock_token}/deadletter` | Dead-letter |
| `PUT` / `GET` / `DELETE /{ns}/topics/{topic}`, `GET /{ns}/topics` | Topic CRUD |
| `PUT /{ns}/topics/{topic}/subscriptions/{sub}`, `GET .../subscriptions` | Subscription CRUD |
| `POST /{ns}/topics/{topic}/messages` | Publish; fans out a copy to every subscription |
| `GET /{ns}/topics/{topic}/subscriptions/{sub}/messages` | Receive from a subscription |

### GCP Cloud Storage (JSON API, `/storage/v1/...`)

Served at `/gcp/...` and at the root, so `client_options={"api_endpoint": "http://localhost:4793"}`
works with `google-cloud-storage`.

- Buckets: insert (409 on duplicates), get, list, delete (409 when not empty).
- Objects: media, multipart, and resumable (chunked) uploads; download via
  `alt=media` on both `/storage/v1/...` and `/download/storage/v1/...` with
  `Range`; get, patch (metadata), delete; list with `prefix`, `delimiter` →
  `prefixes`, `startOffset`/`endOffset`, paging; `ifGenerationMatch` (412) and
  `generation`. Resources include `generation`, `md5Hash`/`crc32c` (base64),
  `mediaLink`, `selfLink`, RFC 3339 timestamps.

### GCP Pub/Sub (REST, `/v1/projects/{project}/...`)

Works with `google-cloud-pubsub` using the REST transport
(`transport="rest"`, `client_options={"api_endpoint": "http://localhost:4793"}`).

Topics: create (409 on duplicates), get, list, delete, list subscriptions.
Subscriptions: create (`ackDeadlineSeconds`), get, list, delete. Messaging:
`:publish` (base64 `data`, `attributes`, returns `messageIds`), `:pull`
(`receivedMessages` with `ackId`, `publishTime`, `deliveryAttempt`; pulled
messages are leased until the ack deadline), `:acknowledge`, `:modifyAckDeadline`.

---

## AWS protocol reference

`POST /` is the single AWS front door (`app.rs::aws_post`):

| Service | Protocol | Content-Type | Dispatch |
|---|---|---|---|
| SES v1, SNS | AWS Query | `application/x-www-form-urlencoded` | `Action` form field (SES list first, then SNS) |
| SQS, DynamoDB | AWS JSON 1.0 | `application/x-amz-json-1.0` | `X-Amz-Target` prefix (`AmazonSQS.`, `DynamoDB_`) |
| Secrets Manager, KMS, Cognito | AWS JSON 1.1 | `application/x-amz-json-1.1` | `X-Amz-Target` prefix (`secretsmanager.`, `TrentService.`, `AWSCognitoIdentityProviderService.`) |

REST services are routed by path: SES v2 (`/v2/email/...`), S3
(`/{bucket}/{key}`, or `{bucket}.s3.localhost`), Bedrock
(`/foundation-models`, `/model/{id}/invoke*`), Cognito OIDC
(`/{poolId}/.well-known/*`, `/oauth2/*`, `/login`, `/logout`).

Error shapes: Query → XML `ErrorResponse`; JSON protocols → `{"__type", "message"}`
(SQS also sets `x-amzn-query-error`); restJson1 (SES v2) → `x-amzn-ErrorType`
header plus `__type`/`message` body (`proto::rest_json_error`); S3 → XML `Error`.

---

## AWS Bedrock simulation

CloudTwin includes a fully config-driven Bedrock simulation engine for testing
application-level Bedrock integration logic without calling real models.

### Module layout

```
src/aws/bedrock/
  mod.rs              # router() entry point + re-exports
  config.rs           # BedrockSimConfig and model/scenario config (`bedrock:` YAML)
  state.rs            # BedrockState – per-model request counters
  generator.rs        # Deterministic text/schema/static generation
  scenario_engine.rs  # Central response-resolution pipeline
  streaming.rs        # EventStream binary encoding + chunking
  handlers.rs         # InvokeModel, InvokeModelWithResponseStream, ListFoundationModels
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

---

## Testing

```bash
make test-unit                 # cargo test
make test-integration          # SDK suites (boto3, Azure, GCP) against target/debug/cloudtwin
make test-integration-sqlite   # same, with a file-backed database
make lint                      # cargo fmt --check + clippy -D warnings
```

**In-process tests** live next to each module (`src/aws/<service>/tests.rs`)
and use `crate::testutil::TestApp`: the real router on an in-memory database,
driven over HTTP exactly like an SDK (`aws_json`, `aws_query`, `rest`, `raw`
request builders). Prefer these for new behaviour.

**SDK integration tests** live in `tests/` and use the official Python SDKs —
boto3 is the most mature AWS SDK, so it is the reference client. The session
fixture in `tests/conftest.py` starts `target/debug/cloudtwin` on a free port
with `tests/cloudtwin-test.yml` and in-memory storage; per-provider
`conftest.py` files expose SDK clients (`s3`, `sqs`, `blob_client`, `gcs`, ...).
Set `CLOUDTWIN_TEST_URL` to target an already-running server, or
`CLOUDTWIN_BIN` to test another binary. Suites for services not yet ported are
kept and marked skipped so they serve as the spec when the service lands.

```python
def test_something(s3):
    s3.create_bucket(Bucket="my-bucket")
    s3.put_object(Bucket="my-bucket", Key="file.txt", Body=b"hello")
    assert s3.get_object(Bucket="my-bucket", Key="file.txt")["Body"].read() == b"hello"
```

The server is shared across a session, so use unique resource names per test
(or call `POST /_cloudtwin/reset`).

---

## Adding a new service

1. Create `src/aws/<service>/` with `mod.rs`, `handlers.rs`, `service.rs`,
   `models.rs`, `schema.rs` (`pub fn migrate(conn)`), and `tests.rs`.
2. Call `<service>::schema::migrate(conn)` from `Database::migrate` in `db.rs`.
3. Wire the protocol: an `X-Amz-Target` prefix or Query `Action` list in
   `app.rs::aws_post`, or merge a REST router in `app::build_router`.
4. Expose `pub async fn seed(state, doc)` and add it to `seed.rs`; add the
   table prefix to `SERVICE_TABLES` in `control.rs`.
5. If other services need to call it, expose a small `*_internal` function
   from `mod.rs` instead of going through HTTP.

---

## Dashboard

The dashboard is a static UI in `dashboard/static/` served at `/dashboard`
when `CLOUDTWIN_DASHBOARD_STATIC` (or `dashboard.static_path`) points at it —
the Docker image and `make run-with-dashboard` do this. Its JSON API lives in
`src/dashboard.rs` under `/api/dashboard/*` and is always mounted; handlers
read SQLite directly.

| Dashboard page | API endpoint |
|---|---|
| Overview | `GET /api/dashboard/health` |
| Event log | `GET /api/dashboard/events?limit=N` |
| Mailbox | `GET /api/dashboard/mailbox`, `GET /api/dashboard/mailbox/{id}` |
| AWS SES / S3 / SNS / SQS | `GET /api/dashboard/aws/{ses,s3,sns,sqs}` |
| AWS Bedrock / DynamoDB / Secrets Manager | `GET /api/dashboard/aws/{bedrock,dynamodb,secretsmanager}` |
| Azure Blob / Service Bus | `GET /api/dashboard/azure/{blob,servicebus}` |
| GCP Storage / Pub/Sub | `GET /api/dashboard/gcp/{storage,pubsub}` |

For UI work, run `make run-with-dashboard` and reload the browser; the UI is
plain JavaScript with no build step.

---

## Not yet implemented

These services existed in the retired Python backend (preserved on the
`archive/python` branch) and have skipped SDK suites in `tests/` waiting for a
Rust port: AWS Lambda, Azure Queue Storage, Event Grid, Key Vault, Functions,
GCP Firestore, Cloud Tasks, Secret Manager, Cloud Functions.

Request signatures are never validated (presigned URLs and POST policies work
because of this), and policies/lifecycle rules are stored but not enforced.
