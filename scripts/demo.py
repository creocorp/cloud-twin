#!/usr/bin/env python3
"""
Demo seed script — populates a running CloudTwin server with realistic
sample data so the dashboard shows meaningful usage for screenshots.

Usage:
    make demo                              # server must be running on port 4793
    python scripts/demo.py
    python scripts/demo.py --url http://localhost:4793

Requires dev dependencies (boto3, azure-storage-blob, google-cloud-*):
    make install-dev
"""

from __future__ import annotations

import argparse
import sys
import time

import httpx

BASE_URL = "http://localhost:4793"

# ── AWS credentials (fake – CloudTwin accepts any value) ─────────────────────

_AWS_CREDS = dict(
    aws_access_key_id="demo",
    aws_secret_access_key="demo",
    region_name="us-east-1",
)

# ── Azure defaults (must match the running server's config) ──────────────────

_AZURE_ACCOUNT = "devstoreaccount1"
_AZURE_KEY = "Eby8vdM02xNOcqFlJdE1SWKvW4GS0IEJSVDMuoFSSjM4="
_AZURE_NAMESPACE = "cloudtwin"

# ── GCP defaults ─────────────────────────────────────────────────────────────

_GCP_PROJECT = "cloudtwin-local"


# ─────────────────────────────────────────────────────────────────────────────
# Helpers
# ─────────────────────────────────────────────────────────────────────────────


def _wait_ready(base: str, timeout: int = 10) -> None:
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            r = httpx.get(f"{base}/_health", timeout=2.0)
            if r.status_code == 200:
                return
        except Exception:
            pass
        time.sleep(0.5)
    print(f"ERROR: CloudTwin is not reachable at {base}/_health", file=sys.stderr)
    sys.exit(1)


def _ok(label: str, resp: httpx.Response | None = None) -> None:
    status = f" [{resp.status_code}]" if resp else ""
    print(f"  ✓ {label}{status}")


def _skip(label: str, reason: str) -> None:
    print(f"  - {label} (skipped: {reason})")


# ─────────────────────────────────────────────────────────────────────────────
# AWS
# ─────────────────────────────────────────────────────────────────────────────


def seed_aws(base: str) -> None:
    try:
        import boto3
    except ImportError:
        _skip("AWS", "boto3 not installed — run: make install-dev")
        return

    print("\n── AWS ──────────────────────────────────────────────────────────────")
    _seed_s3(base, boto3)
    _seed_ses(base, boto3)
    _seed_sns(base, boto3)
    _seed_sqs(base, boto3)
    _seed_bedrock(base, boto3)


def _seed_s3(base: str, boto3) -> None:
    s3 = boto3.client("s3", endpoint_url=base, **_AWS_CREDS)
    for name in ("demo-assets", "demo-logs", "demo-backups"):
        try:
            s3.create_bucket(Bucket=name)
        except Exception:
            pass  # already exists
    # Upload a few objects
    s3.put_object(Bucket="demo-assets", Key="images/logo.png", Body=b"<png-data>")
    s3.put_object(Bucket="demo-assets", Key="css/main.css", Body=b"body{}")
    s3.put_object(Bucket="demo-logs", Key="2026/04/25.log", Body=b"INFO started\n")
    s3.put_object(Bucket="demo-backups", Key="db/backup.sql", Body=b"-- backup")
    _ok("S3 — 3 buckets, 4 objects")


def _seed_ses(base: str, boto3) -> None:
    ses = boto3.client("ses", endpoint_url=base, **_AWS_CREDS)
    sesv2 = boto3.client("sesv2", endpoint_url=base, **_AWS_CREDS)
    for identity in ("alerts@example.com", "noreply@demo.io", "example.com"):
        try:
            ses.verify_email_identity(EmailAddress=identity)
        except Exception:
            pass
    # Send a few emails via v1
    for subject, dest in [
        ("Welcome!", "user1@test.com"),
        ("Your invoice", "user2@test.com"),
        ("Password reset", "user3@test.com"),
    ]:
        try:
            ses.send_email(
                Source="noreply@demo.io",
                Destination={"ToAddresses": [dest]},
                Message={
                    "Subject": {"Data": subject},
                    "Body": {"Text": {"Data": f"Hi, this is a demo email: {subject}"}},
                },
            )
        except Exception:
            pass
    _ok("SES — 3 identities, 3 emails sent")


def _seed_sns(base: str, boto3) -> None:
    sns = boto3.client("sns", endpoint_url=base, **_AWS_CREDS)
    for topic_name in ("order-events", "user-notifications", "audit-log"):
        try:
            resp = sns.create_topic(Name=topic_name)
            arn = resp["TopicArn"]
            sns.subscribe(
                TopicArn=arn,
                Protocol="https",
                Endpoint=f"https://hooks.example.com/{topic_name}",
            )
            sns.publish(TopicArn=arn, Message=f"Demo event on {topic_name}")
        except Exception:
            pass
    _ok("SNS — 3 topics, 3 subscriptions, 3 messages")


def _seed_sqs(base: str, boto3) -> None:
    sqs = boto3.client("sqs", endpoint_url=base, **_AWS_CREDS)
    for queue_name in ("jobs", "notifications", "dead-letter"):
        try:
            resp = sqs.create_queue(QueueName=queue_name)
            url = resp["QueueUrl"]
            for i in range(3):
                sqs.send_message(
                    QueueUrl=url,
                    MessageBody=f'{{"type": "demo", "seq": {i}}}',
                )
        except Exception:
            pass
    _ok("SQS — 3 queues, 9 messages")


def _seed_bedrock(base: str, boto3) -> None:
    bedrock = boto3.client(
        "bedrock-runtime",
        endpoint_url=base,
        **_AWS_CREDS,
    )
    import json as _json

    _MODELS = [
        ("amazon.titan-text-express-v1",           "Hello, give me a one-sentence greeting."),
        ("anthropic.claude-3-haiku-20240307-v1:0", "Summarise cloud computing in one sentence."),
        ("meta.llama3-8b-instruct-v1:0",           "What is the capital of France?"),
    ]
    invocations = 0
    for model_id, prompt in _MODELS:
        for _ in range(2):  # 2 invocations per model → 6 total
            try:
                bedrock.invoke_model(
                    modelId=model_id,
                    body=_json.dumps({"prompt": prompt, "max_tokens": 64}).encode(),
                    contentType="application/json",
                    accept="application/json",
                )
                invocations += 1
            except Exception:
                pass
    _ok(f"Bedrock — {len(_MODELS)} models, {invocations} invocations")


# ─────────────────────────────────────────────────────────────────────────────
# Azure
# ─────────────────────────────────────────────────────────────────────────────


def seed_azure(base: str) -> None:
    print("\n── Azure ────────────────────────────────────────────────────────────")
    _seed_azure_blob(base)
    _seed_azure_servicebus(base)


def _seed_azure_blob(base: str) -> None:
    try:
        from azure.core.credentials import AzureNamedKeyCredential
        from azure.storage.blob import BlobServiceClient
    except ImportError:
        _skip("Azure Blob", "azure-storage-blob not installed — run: make install-dev")
        return

    account_url = f"{base}/{_AZURE_ACCOUNT}"
    credential = AzureNamedKeyCredential(_AZURE_ACCOUNT, _AZURE_KEY)
    client = BlobServiceClient(account_url=account_url, credential=credential)

    for container_name in ("media", "documents", "cache"):
        try:
            client.create_container(container_name)
        except Exception:
            pass

    try:
        media = client.get_container_client("media")
        media.upload_blob("images/hero.jpg", b"\xff\xd8\xff", overwrite=True)
        media.upload_blob("videos/intro.mp4", b"\x00\x00\x00", overwrite=True)
    except Exception:
        pass

    try:
        docs = client.get_container_client("documents")
        docs.upload_blob("reports/q1.pdf", b"%PDF-1.4", overwrite=True)
        docs.upload_blob("contracts/nda.docx", b"PK\x03\x04", overwrite=True)
    except Exception:
        pass

    _ok("Azure Blob — 3 containers, 4 blobs")


def _seed_azure_servicebus(base: str) -> None:
    http = httpx.Client(base_url=base, timeout=10.0)
    ns = _AZURE_NAMESPACE

    # Create queues via the CloudTwin Service Bus REST API
    for queue_name in ("orders", "notifications"):
        try:
            r = http.put(f"/servicebus/{ns}/queues/{queue_name}")
            # Send a few messages
            for i in range(3):
                http.post(
                    f"/servicebus/{ns}/queues/{queue_name}/messages",
                    json={"body": f"demo message {i}", "message_id": f"msg-{i}"},
                )
        except Exception:
            pass

    # Create topics with subscriptions
    for topic_name in ("events", "alerts"):
        try:
            http.put(f"/servicebus/{ns}/topics/{topic_name}")
            http.put(f"/servicebus/{ns}/topics/{topic_name}/subscriptions/all")
        except Exception:
            pass

    _ok("Azure Service Bus — 2 queues, 2 topics, 6 messages")


# ─────────────────────────────────────────────────────────────────────────────
# GCP
# ─────────────────────────────────────────────────────────────────────────────


def seed_gcp(base: str) -> None:
    print("\n── GCP ──────────────────────────────────────────────────────────────")
    _seed_gcs(base)
    _seed_pubsub(base)


def _seed_gcs(base: str) -> None:
    try:
        from google.auth.credentials import AnonymousCredentials
        from google.cloud import storage
    except ImportError:
        _skip("GCP Storage", "google-cloud-storage not installed — run: make install-dev")
        return

    gcs = storage.Client(
        project=_GCP_PROJECT,
        credentials=AnonymousCredentials(),
        client_options={"api_endpoint": base},
    )

    for bucket_name in ("demo-uploads", "demo-static", "demo-archives"):
        try:
            gcs.create_bucket(bucket_name)
        except Exception:
            pass

    try:
        bucket = gcs.bucket("demo-uploads")
        bucket.blob("user-avatars/alice.png").upload_from_string(b"\x89PNG")
        bucket.blob("user-avatars/bob.png").upload_from_string(b"\x89PNG")
    except Exception:
        pass

    try:
        bucket = gcs.bucket("demo-static")
        bucket.blob("index.html").upload_from_string(b"<html></html>")
        bucket.blob("app.js").upload_from_string(b"console.log('hello')")
    except Exception:
        pass

    _ok("GCP Storage — 3 buckets, 4 objects")


def _seed_pubsub(base: str) -> None:
    http = httpx.Client(base_url=base, timeout=10.0)
    project = _GCP_PROJECT

    for topic_name in ("user-events", "order-events", "analytics"):
        full_topic = f"projects/{project}/topics/{topic_name}"
        try:
            http.put(f"/v1/{full_topic}")
        except Exception:
            pass

        sub_name = f"{topic_name}-sub"
        full_sub = f"projects/{project}/subscriptions/{sub_name}"
        try:
            http.put(
                f"/v1/{full_sub}",
                json={"topic": full_topic},
            )
        except Exception:
            pass

        # Publish a few messages
        try:
            import base64

            msgs = [
                {"data": base64.b64encode(f"demo event {i}".encode()).decode()}
                for i in range(3)
            ]
            http.post(f"/v1/{full_topic}:publish", json={"messages": msgs})
        except Exception:
            pass

    _ok("GCP Pub/Sub — 3 topics, 3 subscriptions, 9 messages")


# ─────────────────────────────────────────────────────────────────────────────
# Entry point
# ─────────────────────────────────────────────────────────────────────────────


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Seed CloudTwin with demo data for dashboard screenshots."
    )
    parser.add_argument(
        "--url",
        default=BASE_URL,
        help=f"CloudTwin base URL (default: {BASE_URL})",
    )
    args = parser.parse_args()
    base = args.url.rstrip("/")

    print(f"Connecting to CloudTwin at {base} …")
    _wait_ready(base)
    print("Server is ready.\n")

    seed_aws(base)
    seed_azure(base)
    seed_gcp(base)

    print(f"\nDone! Open the dashboard: {base}/dashboard")


if __name__ == "__main__":
    main()
