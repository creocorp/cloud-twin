"""AWS Secrets Manager — pure business logic."""

from __future__ import annotations

import logging
import uuid
from datetime import datetime, timezone
from typing import Optional

from cloudtwin.core.errors import NotFoundError
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.persistence.models.aws.secretsmanager import Secret, SecretVersion
from cloudtwin.persistence.repositories.aws.secretsmanager import (
    SecretRepository,
    SecretVersionRepository,
)

log = logging.getLogger("cloudtwin.secretsmanager")

_REGION = "us-east-1"
_ACCOUNT_ID = "000000000000"


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


def _arn(name: str) -> str:
    return f"arn:aws:secretsmanager:{_REGION}:{_ACCOUNT_ID}:secret:{name}"


class SecretsManagerService:
    def __init__(
        self,
        secret_repo: SecretRepository,
        version_repo: SecretVersionRepository,
        telemetry: TelemetryEngine,
    ):
        self._secrets = secret_repo
        self._versions = version_repo
        self._telemetry = telemetry

    async def create_secret(
        self,
        name: str,
        secret_string: Optional[str] = None,
        secret_binary: Optional[bytes] = None,
        description: str = "",
    ) -> Secret:
        existing = await self._secrets.get(name)
        if existing:
            return existing
        secret = Secret(name=name, arn=_arn(name), created_at=_now())
        saved = await self._secrets.save(secret)
        if secret_string is not None or secret_binary is not None:
            version = SecretVersion(
                secret_name=name,
                version_id=str(uuid.uuid4()),
                secret_string=secret_string,
                secret_binary=secret_binary,
                created_at=_now(),
            )
            await self._versions.save(version)
        await self._telemetry.emit("aws", "secretsmanager", "create_secret", {"name": name})
        return saved

    async def get_secret_value(
        self,
        name: str,
        version_id: Optional[str] = None,
    ) -> SecretVersion:
        secret = await self._secrets.get(name)
        if not secret:
            raise NotFoundError(f"Secret not found: {name}")
        if version_id:
            version = await self._versions.get_by_version_id(name, version_id)
        else:
            version = await self._versions.get_latest(name)
        if not version:
            raise NotFoundError(f"No version found for secret: {name}")
        return version

    async def put_secret_value(
        self,
        name: str,
        secret_string: Optional[str] = None,
        secret_binary: Optional[bytes] = None,
    ) -> SecretVersion:
        secret = await self._secrets.get(name)
        if not secret:
            raise NotFoundError(f"Secret not found: {name}")
        version = SecretVersion(
            secret_name=name,
            version_id=str(uuid.uuid4()),
            secret_string=secret_string,
            secret_binary=secret_binary,
            created_at=_now(),
        )
        saved = await self._versions.save(version)
        await self._telemetry.emit("aws", "secretsmanager", "put_secret_value", {"name": name})
        return saved

    async def describe_secret(self, name: str) -> Secret:
        secret = await self._secrets.get(name)
        if not secret:
            raise NotFoundError(f"Secret not found: {name}")
        return secret

    async def list_secrets(self) -> list[Secret]:
        return await self._secrets.list_all()

    async def delete_secret(self, name: str) -> None:
        secret = await self._secrets.get(name)
        if not secret:
            raise NotFoundError(f"Secret not found: {name}")
        await self._versions.delete_all(name)
        await self._secrets.delete(name)
        await self._telemetry.emit("aws", "secretsmanager", "delete_secret", {"name": name})
