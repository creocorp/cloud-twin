"""AWS Secrets Manager — abstract repository interfaces."""

from __future__ import annotations

from abc import ABC, abstractmethod
from typing import Optional

from cloudtwin.persistence.models.aws.secretsmanager import Secret, SecretVersion


class SecretRepository(ABC):
    @abstractmethod
    async def get(self, name: str) -> Optional[Secret]: ...
    @abstractmethod
    async def list_all(self) -> list[Secret]: ...
    @abstractmethod
    async def save(self, secret: Secret) -> Secret: ...
    @abstractmethod
    async def delete(self, name: str) -> None: ...


class SecretVersionRepository(ABC):
    @abstractmethod
    async def save(self, version: SecretVersion) -> SecretVersion: ...
    @abstractmethod
    async def get_latest(self, secret_name: str) -> Optional[SecretVersion]: ...
    @abstractmethod
    async def get_by_version_id(self, secret_name: str, version_id: str) -> Optional[SecretVersion]: ...
    @abstractmethod
    async def delete_all(self, secret_name: str) -> None: ...
