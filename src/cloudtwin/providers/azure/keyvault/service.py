"""Azure Key Vault — pure business logic."""

from __future__ import annotations

import logging
import uuid
from datetime import datetime, timezone

from cloudtwin.core.errors import NotFoundError
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.persistence.models.azure.keyvault import KeyVaultSecret
from cloudtwin.persistence.repositories.azure.keyvault import KeyVaultSecretRepository

log = logging.getLogger("cloudtwin.azure.keyvault")


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class KeyVaultService:
    def __init__(self, repo: KeyVaultSecretRepository, telemetry: TelemetryEngine):
        self._repo = repo
        self._telemetry = telemetry

    async def set_secret(self, vault: str, name: str, value: str) -> KeyVaultSecret:
        secret = KeyVaultSecret(
            vault=vault, name=name, value=value,
            version=str(uuid.uuid4()), created_at=_now(),
        )
        saved = await self._repo.save(secret)
        await self._telemetry.emit("azure", "keyvault", "set_secret", {"vault": vault, "name": name})
        return saved

    async def get_secret(self, vault: str, name: str, version: str | None = None) -> KeyVaultSecret:
        if version:
            secret = await self._repo.get_version(vault, name, version)
        else:
            secret = await self._repo.get_latest(vault, name)
        if not secret:
            raise NotFoundError(f"Secret not found: {name}")
        return secret

    async def list_secrets(self, vault: str) -> list[KeyVaultSecret]:
        return await self._repo.list_by_vault(vault)

    async def delete_secret(self, vault: str, name: str) -> None:
        secret = await self._repo.get_latest(vault, name)
        if not secret:
            raise NotFoundError(f"Secret not found: {name}")
        await self._repo.delete_all(vault, name)
        await self._telemetry.emit("azure", "keyvault", "delete_secret", {"vault": vault, "name": name})
