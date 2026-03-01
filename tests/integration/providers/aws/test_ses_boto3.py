"""
Integration tests: AWS SES via boto3.

These tests hit a real uvicorn server and use the boto3 SES client
exactly as application code would – no mocks, no monkeypatching.
"""

from __future__ import annotations

import pytest
from botocore.exceptions import ClientError

# ---------------------------------------------------------------------------
# VerifyDomainIdentity
# ---------------------------------------------------------------------------


def test_verify_domain_identity(ses):
    resp = ses.verify_domain_identity(Domain="boto3-test.example.com")
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200
    token = resp.get("VerificationToken", "")
    assert len(token) > 0


def test_verify_domain_identity_returns_consistent_token(ses):
    """Verifying the same domain twice should return the same token."""
    r1 = ses.verify_domain_identity(Domain="stable-token.example.com")
    r2 = ses.verify_domain_identity(Domain="stable-token.example.com")
    assert r1["VerificationToken"] == r2["VerificationToken"]


# ---------------------------------------------------------------------------
# VerifyEmailIdentity
# ---------------------------------------------------------------------------


def test_verify_email_identity(ses):
    resp = ses.verify_email_identity(EmailAddress="sender@boto3-test.example.com")
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200


# ---------------------------------------------------------------------------
# GetIdentityVerificationAttributes
# ---------------------------------------------------------------------------


def test_get_identity_verification_attributes_verified(ses):
    domain = "attrs-check.example.com"
    ses.verify_domain_identity(Domain=domain)

    resp = ses.get_identity_verification_attributes(Identities=[domain])
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200

    attrs = resp["VerificationAttributes"]
    assert domain in attrs
    assert attrs[domain]["VerificationStatus"] == "Success"
    assert len(attrs[domain].get("VerificationToken", "")) > 0


def test_get_identity_verification_attributes_unknown(ses):
    resp = ses.get_identity_verification_attributes(
        Identities=["never-verified@nobody.com"]
    )
    attrs = resp["VerificationAttributes"]
    entry = attrs.get("never-verified@nobody.com", {})
    assert entry.get("VerificationStatus") == "NotStarted"


def test_get_identity_verification_attributes_multiple(ses):
    ses.verify_domain_identity(Domain="multi-a.test")
    ses.verify_domain_identity(Domain="multi-b.test")

    resp = ses.get_identity_verification_attributes(
        Identities=["multi-a.test", "multi-b.test", "missing.test"]
    )
    attrs = resp["VerificationAttributes"]
    assert attrs["multi-a.test"]["VerificationStatus"] == "Success"
    assert attrs["multi-b.test"]["VerificationStatus"] == "Success"
    assert attrs["missing.test"]["VerificationStatus"] == "NotStarted"


# ---------------------------------------------------------------------------
# ListIdentities
# ---------------------------------------------------------------------------


def test_list_identities_contains_verified_domain(ses):
    domain = "listable.example.com"
    ses.verify_domain_identity(Domain=domain)

    resp = ses.list_identities()
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200
    assert domain in resp["Identities"]


def test_list_identities_by_type_domain(ses):
    ses.verify_domain_identity(Domain="type-filter.example.com")
    ses.verify_email_identity(EmailAddress="addr@type-filter.example.com")

    resp = ses.list_identities(IdentityType="Domain")
    for ident in resp["Identities"]:
        assert "@" not in ident


def test_list_identities_by_type_email(ses):
    ses.verify_email_identity(EmailAddress="email-type@example.com")

    resp = ses.list_identities(IdentityType="EmailAddress")
    for ident in resp["Identities"]:
        assert "@" in ident


# ---------------------------------------------------------------------------
# SendEmail
# ---------------------------------------------------------------------------


def test_send_email_text(ses):
    resp = ses.send_email(
        Source="sender@example.com",
        Destination={"ToAddresses": ["recipient@example.com"]},
        Message={
            "Subject": {"Data": "Hello from boto3"},
            "Body": {"Text": {"Data": "Plain text body"}},
        },
    )
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200
    assert "@cloudtwin.local" in resp["MessageId"]


def test_send_email_html(ses):
    resp = ses.send_email(
        Source="sender@example.com",
        Destination={"ToAddresses": ["recipient@example.com"]},
        Message={
            "Subject": {"Data": "HTML email"},
            "Body": {"Html": {"Data": "<h1>Hello</h1>"}},
        },
    )
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200
    assert "@cloudtwin.local" in resp["MessageId"]


def test_send_email_text_and_html(ses):
    resp = ses.send_email(
        Source="sender@example.com",
        Destination={
            "ToAddresses": ["to@example.com"],
            "CcAddresses": ["cc@example.com"],
            "BccAddresses": ["bcc@example.com"],
        },
        Message={
            "Subject": {"Data": "Multipart"},
            "Body": {
                "Text": {"Data": "plain"},
                "Html": {"Data": "<p>html</p>"},
            },
        },
    )
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200


def test_send_email_multiple_recipients(ses):
    resp = ses.send_email(
        Source="bulk@example.com",
        Destination={
            "ToAddresses": [
                "alice@example.com",
                "bob@example.com",
                "carol@example.com",
            ]
        },
        Message={
            "Subject": {"Data": "Bulk send"},
            "Body": {"Text": {"Data": "Hi all"}},
        },
    )
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200


def test_send_email_each_call_produces_unique_message_id(ses):
    kwargs = dict(
        Source="sender@example.com",
        Destination={"ToAddresses": ["r@example.com"]},
        Message={"Subject": {"Data": "Dup"}, "Body": {"Text": {"Data": "x"}}},
    )
    id1 = ses.send_email(**kwargs)["MessageId"]
    id2 = ses.send_email(**kwargs)["MessageId"]
    assert id1 != id2
