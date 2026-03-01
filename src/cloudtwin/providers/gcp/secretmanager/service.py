"""GCP Secret Manager — pure business logic."""

from __future__ import annotations

import logging
import uuid
from datetime import datetime, timezone

from cloudtwin.core.errors import NotFoundError
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.persistence.models.gcp.secretmanager import GcpSecret, GcpSecretVersion
from cloudtwin.persistence.repositories.gcp.secretmanager import (
    GcpSecretRepository,
    GcpSecretVersionRepository,
)

log = logging.getLogger("cloudtwin.gcp.secretmanager")


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


def _full_name(project: str, name: str) -> str:
    return f"projects/{project}/secrets/{name}"


class GcpSecretManagerService:
    def __init__(
        self,
        secret_repo: GcpSecretRepository,
        version_repo: GcpSecretVersionRepository,
        telemetry: TelemetryEngine,
    ):
        self._secrets = secret_repo
        self._versions = version_repo
        self._telemetry = telemetry

    async def create_secret(self, project: str, name: str) -> GcpSecret:
        full_name = _full_name(project, name)
        existing = await self._secrets.get(full_name)
        if existing:
            return existing
        secret = GcpSecret(
            project=project, name=name, full_name=full_name, created_at=_now()
        )
        saved = await self._secrets.save(secret)
        await self._telemetry.emit(
            "gcp", "secretmanager", "create_secret", {"project": project, "name": name}
        )
        return saved

    async def add_secret_version(
        self, project: str, name: str, payload: bytes
    ) -> GcpSecretVersion:
        full_name = _full_name(project, name)
        secret = await self._secrets.get(full_name)
        if not secret:
            raise NotFoundError(f"Secret not found: {full_name}")
        version = GcpSecretVersion(
            secret_full_name=full_name,
            version_id=str(uuid.uuid4()),
            payload=payload,
            state="enabled",
            created_at=_now(),
        )
        saved = await self._versions.save(version)
        await self._telemetry.emit(
            "gcp", "secretmanager", "add_secret_version", {"name": name}
        )
        return saved

    async def access_secret_version(
        self, project: str, name: str, version_id: str = "latest"
    ) -> GcpSecretVersion:
        full_name = _full_name(project, name)
        if version_id == "latest":
            version = await self._versions.get_latest(full_name)
        else:
            version = await self._versions.get_by_version_id(full_name, version_id)
        if not version:
            raise NotFoundError(f"Secret version not found: {full_name}/{version_id}")
        return version

    async def list_secrets(self, project: str) -> list[GcpSecret]:
        return await self._secrets.list_by_project(project)

    async def delete_secret(self, project: str, name: str) -> None:
        full_name = _full_name(project, name)
        secret = await self._secrets.get(full_name)
        if not secret:
            raise NotFoundError(f"Secret not found: {full_name}")
        await self._versions.delete_all(full_name)
        await self._secrets.delete(full_name)
        await self._telemetry.emit(
            "gcp", "secretmanager", "delete_secret", {"name": name}
        )
