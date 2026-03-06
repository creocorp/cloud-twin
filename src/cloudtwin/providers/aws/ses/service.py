"""
SES domain service.

Business logic for SES operations. Has no knowledge of HTTP or XML.
Depends on repository interfaces only.
"""

from __future__ import annotations

import secrets
import uuid
from datetime import datetime, timezone

from cloudtwin.config import SesConfig
from cloudtwin.core.errors import IdentityNotVerifiedError
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.persistence.models import SesIdentity, SesMessage
from cloudtwin.persistence.repositories import (
    SesIdentityRepository,
    SesMessageRepository,
)


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class SesService:
    def __init__(
        self,
        config: SesConfig,
        identity_repo: SesIdentityRepository,
        message_repo: SesMessageRepository,
        telemetry: TelemetryEngine,
    ):
        self._config = config
        self._identity_repo = identity_repo
        self._message_repo = message_repo
        self._telemetry = telemetry

    # -------------------------------------------------------------------
    # Identity management
    # -------------------------------------------------------------------

    async def verify_domain_identity(self, domain: str) -> str:
        """Marks a domain as verified. Returns the token (stable across repeated calls)."""
        existing = await self._identity_repo.get(domain)
        if existing and existing.token:
            return existing.token

        token = secrets.token_hex(16)
        identity = SesIdentity(
            id=None,
            identity=domain,
            type="domain",
            verified=True,
            token=token,
            created_at=_now(),
        )
        await self._identity_repo.save(identity)
        await self._telemetry.emit("aws", "ses", "verify_domain", {"domain": domain})
        return token

    async def verify_email_identity(self, email: str) -> None:
        """Records an email identity (immediately verified in permissive mode)."""
        identity = SesIdentity(
            id=None,
            identity=email,
            type="email",
            verified=True,
            token=None,
            created_at=_now(),
        )
        await self._identity_repo.save(identity)
        await self._telemetry.emit("aws", "ses", "verify_email", {"email": email})

    async def get_identity_verification_attributes(
        self, identities: list[str]
    ) -> dict[str, dict]:
        result = {}
        for ident in identities:
            record = await self._identity_repo.get(ident)
            if record:
                result[ident] = {
                    "VerificationStatus": "Success" if record.verified else "Pending",
                    "VerificationToken": record.token or "",
                }
            else:
                result[ident] = {
                    "VerificationStatus": "NotStarted",
                    "VerificationToken": "",
                }
        return result

    async def get_identity(self, identity: str) -> SesIdentity | None:
        return await self._identity_repo.get(identity)

    async def list_identities(self, identity_type: str | None = None) -> list[str]:
        records = await self._identity_repo.list_all()
        if identity_type:
            records = [r for r in records if r.type == identity_type.lower()]
        return [r.identity for r in records]

    async def list_all_identities(self) -> list[SesIdentity]:
        return await self._identity_repo.list_all()

    async def delete_identity(self, identity: str) -> None:
        await self._identity_repo.delete(identity)
        await self._telemetry.emit(
            "aws", "ses", "delete_identity", {"identity": identity}
        )

    # -------------------------------------------------------------------
    # Email sending
    # -------------------------------------------------------------------

    async def send_email(
        self,
        source: str,
        to_addresses: list[str],
        cc_addresses: list[str],
        bcc_addresses: list[str],
        subject: str,
        text_body: str | None,
        html_body: str | None,
    ) -> str:
        if self._config.strict_verification:
            record = await self._identity_repo.get(source)
            if not record or not record.verified:
                # Try domain match
                domain = source.split("@")[-1] if "@" in source else source
                domain_record = await self._identity_repo.get(domain)
                if not domain_record or not domain_record.verified:
                    raise IdentityNotVerifiedError(source)

        all_destinations = to_addresses + cc_addresses + bcc_addresses
        message_id = f"{uuid.uuid4()}@cloudtwin.local"

        message = SesMessage(
            id=None,
            message_id=message_id,
            source=source,
            destinations=all_destinations,
            subject=subject,
            text_body=text_body,
            html_body=html_body,
            raw_mime=None,
            status="sent",
            error_message=None,
            created_at=_now(),
        )
        await self._message_repo.save(message)
        await self._telemetry.emit(
            "aws",
            "ses",
            "send_email",
            {
                "source": source,
                "destinations": all_destinations,
                "message_id": message_id,
            },
        )
        return message_id
