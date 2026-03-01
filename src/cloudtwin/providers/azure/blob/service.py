"""Azure Blob Storage – pure business logic."""

from __future__ import annotations

import logging
from datetime import datetime, timezone
from typing import Optional

from cloudtwin.core.errors import ConflictError, NotFoundError
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.persistence.models.azure import AzureBlob, AzureContainer
from cloudtwin.persistence.repositories.azure import (
    AzureBlobRepository,
    AzureContainerRepository,
)

log = logging.getLogger("cloudtwin.azure.blob")


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class BlobService:
    def __init__(
        self,
        account_name: str,
        container_repo: AzureContainerRepository,
        blob_repo: AzureBlobRepository,
        telemetry: TelemetryEngine,
    ):
        self._account = account_name
        self._containers = container_repo
        self._blobs = blob_repo
        self._telemetry = telemetry

    # ------------------------------------------------------------------
    # Containers
    # ------------------------------------------------------------------

    async def list_containers(self) -> list[AzureContainer]:
        return await self._containers.list_by_account(self._account)

    async def create_container(self, name: str) -> AzureContainer:
        existing = await self._containers.get(self._account, name)
        if existing:
            return existing
        container = AzureContainer(account=self._account, name=name, created_at=_now())
        result = await self._containers.save(container)
        await self._telemetry.emit(
            "azure", "blob", "create_container", {"container": name}
        )
        return result

    async def delete_container(self, name: str) -> None:
        existing = await self._containers.get(self._account, name)
        if not existing:
            raise NotFoundError(f"Container not found: {name}")
        await self._containers.delete(self._account, name)
        await self._telemetry.emit(
            "azure", "blob", "delete_container", {"container": name}
        )

    async def get_container(self, name: str) -> AzureContainer:
        container = await self._containers.get(self._account, name)
        if not container:
            raise NotFoundError(f"Container not found: {name}")
        return container

    # ------------------------------------------------------------------
    # Blobs
    # ------------------------------------------------------------------

    async def list_blobs(
        self, container_name: str, prefix: str = ""
    ) -> list[AzureBlob]:
        container = await self.get_container(container_name)
        return await self._blobs.list_by_container(container.id, prefix=prefix)

    async def put_blob(
        self,
        container_name: str,
        blob_name: str,
        data: bytes,
        content_type: str = "application/octet-stream",
        metadata: Optional[dict] = None,
    ) -> AzureBlob:
        container = await self.get_container(container_name)
        import json

        blob = AzureBlob(
            container_id=container.id,
            name=blob_name,
            content_type=content_type,
            content_length=len(data),
            data=data,
            metadata=json.dumps(metadata or {}),
            created_at=_now(),
        )
        result = await self._blobs.save(blob)
        await self._telemetry.emit(
            "azure",
            "blob",
            "put_blob",
            {"container": container_name, "blob": blob_name, "size": len(data)},
        )
        return result

    async def get_blob(self, container_name: str, blob_name: str) -> AzureBlob:
        container = await self.get_container(container_name)
        blob = await self._blobs.get(container.id, blob_name)
        if not blob:
            raise NotFoundError(f"Blob not found: {container_name}/{blob_name}")
        return blob

    async def delete_blob(self, container_name: str, blob_name: str) -> None:
        container = await self.get_container(container_name)
        blob = await self._blobs.get(container.id, blob_name)
        if not blob:
            raise NotFoundError(f"Blob not found: {container_name}/{blob_name}")
        await self._blobs.delete(container.id, blob_name)
        await self._telemetry.emit(
            "azure",
            "blob",
            "delete_blob",
            {"container": container_name, "blob": blob_name},
        )
