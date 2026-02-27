"""
SES HTTP request handlers.

Parses AWS Query protocol params → calls SesService → returns XML.
"""

from __future__ import annotations

from xml.etree.ElementTree import SubElement

from fastapi import Request, Response

from cloudtwin.core.xml import ses_error_response, ses_response
from cloudtwin.providers.aws.ses.service import SesService
from cloudtwin.providers.aws.ses.smtp import relay_email
from cloudtwin.config import SesConfig


def register_ses_handlers(router, config: SesConfig, service: SesService):
    """Register all SES action handlers onto a QueryProtocolRouter."""

    # ------------------------------------------------------------------
    # VerifyDomainIdentity
    # ------------------------------------------------------------------
    async def handle_verify_domain_identity(request: Request, params: dict) -> Response:
        domain = params.get("Domain")
        if not domain:
            return Response(
                content=ses_error_response("MissingParameter", "Domain is required"),
                status_code=400,
                media_type="text/xml",
            )
        token = await service.verify_domain_identity(domain)

        def build(result):
            SubElement(result, "VerificationToken").text = token

        return Response(
            content=ses_response("VerifyDomainIdentity", build),
            media_type="text/xml",
        )

    # ------------------------------------------------------------------
    # VerifyEmailIdentity
    # ------------------------------------------------------------------
    async def handle_verify_email_identity(request: Request, params: dict) -> Response:
        email = params.get("EmailAddress")
        if not email:
            return Response(
                content=ses_error_response("MissingParameter", "EmailAddress is required"),
                status_code=400,
                media_type="text/xml",
            )
        await service.verify_email_identity(email)

        return Response(
            content=ses_response("VerifyEmailIdentity", lambda r: None),
            media_type="text/xml",
        )

    # ------------------------------------------------------------------
    # GetIdentityVerificationAttributes
    # ------------------------------------------------------------------
    async def handle_get_identity_verification_attributes(
        request: Request, params: dict
    ) -> Response:
        # Identities passed as Identities.member.1, Identities.member.2, …
        identities = [
            v for k, v in params.items() if k.startswith("Identities.member.")
        ]
        attrs = await service.get_identity_verification_attributes(identities)

        def build(result):
            entries = SubElement(result, "VerificationAttributes")
            for identity, attr in attrs.items():
                entry = SubElement(entries, "entry")
                SubElement(entry, "key").text = identity
                val = SubElement(entry, "value")
                SubElement(val, "VerificationStatus").text = attr["VerificationStatus"]
                if attr["VerificationToken"]:
                    SubElement(val, "VerificationToken").text = attr["VerificationToken"]

        return Response(
            content=ses_response("GetIdentityVerificationAttributes", build),
            media_type="text/xml",
        )

    # ------------------------------------------------------------------
    # ListIdentities
    # ------------------------------------------------------------------
    async def handle_list_identities(request: Request, params: dict) -> Response:
        identity_type = params.get("IdentityType")
        identities = await service.list_identities(identity_type)

        def build(result):
            members = SubElement(result, "Identities")
            for ident in identities:
                SubElement(members, "member").text = ident

        return Response(
            content=ses_response("ListIdentities", build),
            media_type="text/xml",
        )

    # ------------------------------------------------------------------
    # SendEmail
    # ------------------------------------------------------------------
    async def handle_send_email(request: Request, params: dict) -> Response:
        source = params.get("Source")
        subject = params.get("Message.Subject.Data", "")
        text_body = params.get("Message.Body.Text.Data")
        html_body = params.get("Message.Body.Html.Data")

        # Collect To/CC/BCC
        to_addresses = [v for k, v in params.items() if k.startswith("Destination.ToAddresses.member.")]
        cc_addresses = [v for k, v in params.items() if k.startswith("Destination.CcAddresses.member.")]
        bcc_addresses = [v for k, v in params.items() if k.startswith("Destination.BccAddresses.member.")]

        if not source:
            return Response(
                content=ses_error_response("MissingParameter", "Source is required"),
                status_code=400,
                media_type="text/xml",
            )
        if not to_addresses:
            return Response(
                content=ses_error_response("MissingParameter", "At least one ToAddress is required"),
                status_code=400,
                media_type="text/xml",
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
        except Exception as exc:
            from cloudtwin.core.errors import CloudTwinError

            if isinstance(exc, CloudTwinError):
                return Response(
                    content=ses_error_response(exc.code, exc.message),
                    status_code=exc.http_status,
                    media_type="text/xml",
                )
            raise

        # Optional SMTP relay
        if config.smtp.enabled:
            await relay_email(
                config=config.smtp,
                source=source,
                destinations=to_addresses + cc_addresses + bcc_addresses,
                subject=subject,
                text_body=text_body,
                html_body=html_body,
            )

        def build(result):
            SubElement(result, "MessageId").text = message_id

        return Response(
            content=ses_response("SendEmail", build),
            media_type="text/xml",
        )

    # ------------------------------------------------------------------
    # Register
    # ------------------------------------------------------------------
    router.register("VerifyDomainIdentity", handle_verify_domain_identity)
    router.register("VerifyEmailIdentity", handle_verify_email_identity)
    router.register("GetIdentityVerificationAttributes", handle_get_identity_verification_attributes)
    router.register("ListIdentities", handle_list_identities)
    router.register("SendEmail", handle_send_email)
