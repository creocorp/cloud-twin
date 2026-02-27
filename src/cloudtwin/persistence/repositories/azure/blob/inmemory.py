"""Azure Blob Storage — in-memory repository implementations."""

from __future__ import annotations

from typing import Optional

from cloudtwin.persistence.models.azure.blob import AzureBlob, AzureContainer
from cloudtwin.persistence.repositories.azure.blob.repository import (
    AzureBlobRepository,
    AzureContainerRepository,
)


class InMemoryAzureContainerRepository(AzureContainerRepository):
    def __init__(self):
        self._store: dict[tuple[str, str], AzureContainer] = {}
        self._next_id = 1

    async def get(self, account: str, name: str) -> Optional[AzureContainer]:
        return self._store.get((account, name))

    async def list_by_account(self, account: str) -> list[AzureContainer]:
        return [c for (a, _), c in self._store.items() if a == account]

    async def save(self, container: AzureContainer) -> AzureContainer:
        key = (container.account, container.name)
        if key not in self._store:
            container.id = self._next_id
            self._next_id += 1
        self._store[key] = container
        return container

    async def delete(self, account: str, name: str) -> None:
        self._store.pop((account, name), None)


class InMemoryAzureBlobRepository(AzureBlobRepository):
    def __init__(self):
        self._store: dict[tuple[int, str], AzureBlob] = {}

    async def get(self, container_id: int, name: str) -> Optional[AzureBlob]:
        return self._store.get((container_id, name))

    async def list_by_container(self, container_id: int, prefix: str = "") -> list[AzureBlob]:
        return [b for (cid, k), b in self._store.items() if cid == container_id and k.startswith(prefix)]

    async def save(self, blob: AzureBlob) -> AzureBlob:
        self._store[(blob.container_id, blob.name)] = blob
        return blob

    async def delete(self, container_id: int, name: str) -> None:
        self._store.pop((container_id, name), None)
