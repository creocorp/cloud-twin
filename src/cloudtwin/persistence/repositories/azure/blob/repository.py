"""Azure Blob Storage — abstract repository interfaces."""

from __future__ import annotations

from abc import ABC, abstractmethod
from typing import Optional

from cloudtwin.persistence.models.azure.blob import AzureBlob, AzureContainer


class AzureContainerRepository(ABC):
    @abstractmethod
    async def get(self, account: str, name: str) -> Optional[AzureContainer]: ...
    @abstractmethod
    async def list_by_account(self, account: str) -> list[AzureContainer]: ...
    @abstractmethod
    async def save(self, container: AzureContainer) -> AzureContainer: ...
    @abstractmethod
    async def delete(self, account: str, name: str) -> None: ...


class AzureBlobRepository(ABC):
    @abstractmethod
    async def get(self, container_id: int, name: str) -> Optional[AzureBlob]: ...
    @abstractmethod
    async def list_by_container(
        self, container_id: int, prefix: str = ""
    ) -> list[AzureBlob]: ...
    @abstractmethod
    async def save(self, blob: AzureBlob) -> AzureBlob: ...
    @abstractmethod
    async def delete(self, container_id: int, name: str) -> None: ...
