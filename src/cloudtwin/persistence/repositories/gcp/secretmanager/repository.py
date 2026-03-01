"""GCP Secret Manager — abstract repository interfaces."""

from __future__ import annotations

from abc import ABC, abstractmethod
from typing import Optional

from cloudtwin.persistence.models.gcp.secretmanager import GcpSecret, GcpSecretVersion


class GcpSecretRepository(ABC):
    @abstractmethod
    async def get(self, full_name: str) -> Optional[GcpSecret]: ...
    @abstractmethod
    async def list_by_project(self, project: str) -> list[GcpSecret]: ...
    @abstractmethod
    async def save(self, secret: GcpSecret) -> GcpSecret: ...
    @abstractmethod
    async def delete(self, full_name: str) -> None: ...


class GcpSecretVersionRepository(ABC):
    @abstractmethod
    async def save(self, version: GcpSecretVersion) -> GcpSecretVersion: ...
    @abstractmethod
    async def get_latest(self, secret_full_name: str) -> Optional[GcpSecretVersion]: ...
    @abstractmethod
    async def get_by_version_id(
        self, secret_full_name: str, version_id: str
    ) -> Optional[GcpSecretVersion]: ...
    @abstractmethod
    async def list_by_secret(self, secret_full_name: str) -> list[GcpSecretVersion]: ...
    @abstractmethod
    async def delete_all(self, secret_full_name: str) -> None: ...
