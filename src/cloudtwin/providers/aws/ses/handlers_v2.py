"""
SES v2 REST/JSON handlers.

Mounted at /v2 – backed by the same SesService as the v1 Query handlers.

Routes (matching botocore sesv2 service model):
  POST   /v2/email/identities              → CreateEmailIdentity
  GET    /v2/email/identities              → ListEmailIdentities
  GET    /v2/email/identities/{identity}   → GetEmailIdentity
  DELETE /v2/email/identities/{identity}   → DeleteEmailIdentity
  POST   /v2/email/outbound-emails         → SendEmail
"""

from __future__ import annotations

from fastapi import APIRouter, Request
from fastapi.responses import JSONResponse

from cloudtwin.config import SesConfig
from cloudtwin.core.errors import CloudTwinError
from cloudtwin.providers.aws.ses.service import SesService
from cloudtwin.providers.aws.ses.smtp import relay_email


def _identity_type_v2(type_: str) -> str:
    return "DOMAIN" if type_ == "domain" else "EMAIL_ADDRESS"


def _verification_status_v2(verified: bool) -> str:
    return "SUCCESS" if verified else "PENDING"


def make_sesv2_router(config: SesConfig, service: SesService) -> APIRouter:
    router = APIRouter()

    # ------------------------------------------------------------------
    # CreateEmailIdentity  POST /v2/email/identities
    # ------------------------------------------------------------------
    @router.post("/email/identities", status_code=200)
    async def create_email_identity(request: Request):
        body = await request.json()
        email_identity = body.get("EmailIdentity")
        if not email_identity:
            return JSONResponse(
                {"message": "EmailIdentity is required"}, status_code=400
            )

        # Treat strings without "@" as domain identities
        if "@" not in email_identity:
            await service.verify_domain_identity(email_identity)
            identity_type = "DOMAIN"
        else:
            await service.verify_email_identity(email_identity)
            identity_type = "EMAIL_ADDRESS"

        return JSONResponse(
            {
                "IdentityType": identity_type,
                "VerifiedForSendingStatus": True,
                "VerificationStatus": "SUCCESS",
            }
        )

    # ------------------------------------------------------------------
    # ListEmailIdentities  GET /v2/email/identities
    # ------------------------------------------------------------------
    @router.get("/email/identities", status_code=200)
    async def list_email_identities():
        records = await service.list_all_identities()
        return JSONResponse(
            {
                "EmailIdentities": [
                    {
                        "IdentityName": r.identity,
                        "IdentityType": _identity_type_v2(r.type),
                        "VerifiedForSendingStatus": r.verified,
                    }
                    for r in records
                ]
            }
        )

    # ------------------------------------------------------------------
    # GetEmailIdentity  GET /v2/email/identities/{EmailIdentity}
    # ------------------------------------------------------------------
    @router.get("/email/identities/{email_identity:path}", status_code=200)
    async def get_email_identity(email_identity: str):
        record = await service.get_identity(email_identity)
        if record is None:
            return JSONResponse(
                {"message": f"Identity does not exist: {email_identity}"},
                status_code=404,
            )
        return JSONResponse(
            {
                "IdentityType": _identity_type_v2(record.type),
                "VerifiedForSendingStatus": record.verified,
                "VerificationStatus": _verification_status_v2(record.verified),
                "FeedbackForwardingStatus": True,
            }
        )

    # ------------------------------------------------------------------
    # DeleteEmailIdentity  DELETE /v2/email/identities/{EmailIdentity}
    # ------------------------------------------------------------------
    @router.delete("/email/identities/{email_identity:path}", status_code=200)
    async def delete_email_identity(email_identity: str):
        await service.delete_identity(email_identity)
        return JSONResponse({})

    # ------------------------------------------------------------------
    # SendEmail  POST /v2/email/outbound-emails
    # ------------------------------------------------------------------
    @router.post("/email/outbound-emails", status_code=200)
    async def send_email(request: Request):
        body = await request.json()

        source = body.get("FromEmailAddress", "")
        destination = body.get("Destination", {})
        to_addresses = destination.get("ToAddresses", [])
        cc_addresses = destination.get("CcAddresses", [])
        bcc_addresses = destination.get("BccAddresses", [])

        content = body.get("Content", {})
        simple = content.get("Simple", {})
        subject = simple.get("Subject", {}).get("Data", "")
        body_content = simple.get("Body", {})
        text_body = body_content.get("Text", {}).get("Data")
        html_body = body_content.get("Html", {}).get("Data")

        if not source:
            return JSONResponse(
                {"message": "FromEmailAddress is required"}, status_code=400
            )
        if not to_addresses:
            return JSONResponse(
                {"message": "At least one ToAddress is required"}, status_code=400
            )

        try:
            message_id = await service.send_email(
                source=source,
                to_addresses=to_addresses,
                cc_addresses=cc_addresses,
                bcc_addresses=bcc_addresses,
                subject=subject,
                text_body=text_body,
                html_body=html_body,
            )
        except CloudTwinError as exc:
            return JSONResponse({"message": exc.message}, status_code=exc.http_status)

        if config.smtp.enabled:
            await relay_email(
                config=config.smtp,
                source=source,
                destinations=to_addresses + cc_addresses + bcc_addresses,
                subject=subject,
                text_body=text_body,
                html_body=html_body,
            )

        return JSONResponse({"MessageId": message_id})

    return router
