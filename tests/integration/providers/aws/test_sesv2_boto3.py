"""
Integration tests: AWS SES v2 via boto3.

All tests share a single in-memory server (session fixture).
Identity names are unique per test to avoid cross-test interference.
"""

from __future__ import annotations

import pytest
from botocore.exceptions import ClientError

# ---------------------------------------------------------------------------
# CreateEmailIdentity
# ---------------------------------------------------------------------------


def test_create_domain_identity(sesv2):
    resp = sesv2.create_email_identity(EmailIdentity="v2-domain.example.com")
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200
    assert resp["IdentityType"] == "DOMAIN"
    assert resp["VerifiedForSendingStatus"] is True


def test_create_email_address_identity(sesv2):
    resp = sesv2.create_email_identity(EmailIdentity="sender@v2-test.example.com")
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200
    assert resp["IdentityType"] == "EMAIL_ADDRESS"
    assert resp["VerifiedForSendingStatus"] is True


def test_create_identity_idempotent(sesv2):
    """Creating the same identity twice should succeed and be consistent."""
    sesv2.create_email_identity(EmailIdentity="idempotent-v2.example.com")
    resp = sesv2.create_email_identity(EmailIdentity="idempotent-v2.example.com")
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200


# ---------------------------------------------------------------------------
# GetEmailIdentity
# ---------------------------------------------------------------------------


def test_get_domain_identity(sesv2):
    domain = "get-check-v2.example.com"
    sesv2.create_email_identity(EmailIdentity=domain)

    resp = sesv2.get_email_identity(EmailIdentity=domain)
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200
    assert resp["IdentityType"] == "DOMAIN"
    assert resp["VerifiedForSendingStatus"] is True
    assert resp["VerificationStatus"] == "SUCCESS"


def test_get_email_address_identity(sesv2):
    addr = "getme@v2-test.example.com"
    sesv2.create_email_identity(EmailIdentity=addr)

    resp = sesv2.get_email_identity(EmailIdentity=addr)
    assert resp["IdentityType"] == "EMAIL_ADDRESS"
    assert resp["VerifiedForSendingStatus"] is True


def test_get_nonexistent_identity_raises(sesv2):
    with pytest.raises(ClientError) as exc_info:
        sesv2.get_email_identity(EmailIdentity="nobody@nowhere-v2.example.com")
    assert exc_info.value.response["ResponseMetadata"]["HTTPStatusCode"] == 404


# ---------------------------------------------------------------------------
# ListEmailIdentities
# ---------------------------------------------------------------------------


def test_list_identities_contains_created(sesv2):
    domain = "listable-v2.example.com"
    sesv2.create_email_identity(EmailIdentity=domain)

    resp = sesv2.list_email_identities()
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200
    names = [e["IdentityName"] for e in resp.get("EmailIdentities", [])]
    assert domain in names


def test_list_identities_includes_type(sesv2):
    sesv2.create_email_identity(EmailIdentity="typed-domain-v2.example.com")
    sesv2.create_email_identity(EmailIdentity="typed-email@v2.example.com")

    resp = sesv2.list_email_identities()
    by_name = {e["IdentityName"]: e for e in resp.get("EmailIdentities", [])}

    assert by_name["typed-domain-v2.example.com"]["IdentityType"] == "DOMAIN"
    assert by_name["typed-email@v2.example.com"]["IdentityType"] == "EMAIL_ADDRESS"


# ---------------------------------------------------------------------------
# DeleteEmailIdentity
# ---------------------------------------------------------------------------


def test_delete_identity(sesv2):
    domain = "delete-me-v2.example.com"
    sesv2.create_email_identity(EmailIdentity=domain)

    resp = sesv2.delete_email_identity(EmailIdentity=domain)
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200

    with pytest.raises(ClientError) as exc_info:
        sesv2.get_email_identity(EmailIdentity=domain)
    assert exc_info.value.response["ResponseMetadata"]["HTTPStatusCode"] == 404


def test_delete_nonexistent_identity_is_idempotent(sesv2):
    """Deleting an identity that doesn't exist should not raise."""
    resp = sesv2.delete_email_identity(EmailIdentity="ghost-v2.example.com")
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200


# ---------------------------------------------------------------------------
# SendEmail
# ---------------------------------------------------------------------------


def test_send_email_text(sesv2):
    resp = sesv2.send_email(
        FromEmailAddress="sender@v2.example.com",
        Destination={"ToAddresses": ["recipient@v2.example.com"]},
        Content={
            "Simple": {
                "Subject": {"Data": "Hello from SESv2"},
                "Body": {"Text": {"Data": "Plain text via v2"}},
            }
        },
    )
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200
    assert "@cloudtwin.local" in resp["MessageId"]


def test_send_email_html(sesv2):
    resp = sesv2.send_email(
        FromEmailAddress="sender@v2.example.com",
        Destination={"ToAddresses": ["recipient@v2.example.com"]},
        Content={
            "Simple": {
                "Subject": {"Data": "HTML via SESv2"},
                "Body": {"Html": {"Data": "<h1>Hello</h1>"}},
            }
        },
    )
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200


def test_send_email_text_and_html(sesv2):
    resp = sesv2.send_email(
        FromEmailAddress="sender@v2.example.com",
        Destination={
            "ToAddresses": ["to@v2.example.com"],
            "CcAddresses": ["cc@v2.example.com"],
            "BccAddresses": ["bcc@v2.example.com"],
        },
        Content={
            "Simple": {
                "Subject": {"Data": "Multipart v2"},
                "Body": {
                    "Text": {"Data": "plain"},
                    "Html": {"Data": "<p>html</p>"},
                },
            }
        },
    )
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200


def test_send_email_multiple_recipients(sesv2):
    resp = sesv2.send_email(
        FromEmailAddress="bulk@v2.example.com",
        Destination={
            "ToAddresses": ["a@v2.example.com", "b@v2.example.com", "c@v2.example.com"]
        },
        Content={
            "Simple": {
                "Subject": {"Data": "Bulk v2"},
                "Body": {"Text": {"Data": "Hi all"}},
            }
        },
    )
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200


def test_send_email_unique_message_ids(sesv2):
    kwargs = dict(
        FromEmailAddress="sender@v2.example.com",
        Destination={"ToAddresses": ["r@v2.example.com"]},
        Content={
            "Simple": {
                "Subject": {"Data": "Dup v2"},
                "Body": {"Text": {"Data": "x"}},
            }
        },
    )
    id1 = sesv2.send_email(**kwargs)["MessageId"]
    id2 = sesv2.send_email(**kwargs)["MessageId"]
    assert id1 != id2


# ---------------------------------------------------------------------------
# Cross-version identity sharing
# ---------------------------------------------------------------------------


def test_v1_identity_visible_via_v2(ses, sesv2):
    """An identity created via SESv1 should be visible through SESv2."""
    ses.verify_domain_identity(Domain="cross-v1-to-v2.example.com")

    resp = sesv2.get_email_identity(EmailIdentity="cross-v1-to-v2.example.com")
    assert resp["VerificationStatus"] == "SUCCESS"


def test_v2_identity_visible_via_v1(ses, sesv2):
    """An identity created via SESv2 should be listed through SESv1."""
    sesv2.create_email_identity(EmailIdentity="cross-v2-to-v1.example.com")

    resp = ses.list_identities()
    assert "cross-v2-to-v1.example.com" in resp["Identities"]
