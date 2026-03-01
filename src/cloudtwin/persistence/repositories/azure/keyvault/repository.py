"""Azure Key Vault — abstract repository interfaces."""

from __future__ import annotations

from abc import ABC, abstractmethod
from typing import Optional

from cloudtwin.persistence.models.azure.keyvault import KeyVaultSecret


class KeyVaultSecretRepository(ABC):
    @abstractmethod
    async def get_latest(self, vault: str, name: str) -> Optional[KeyVaultSecret]: ...
    @abstractmethod
    async def get_version(
        self, vault: str, name: str, version: str
    ) -> Optional[KeyVaultSecret]: ...
    @abstractmethod
    async def list_by_vault(self, vault: str) -> list[KeyVaultSecret]: ...
    @abstractmethod
    async def save(self, secret: KeyVaultSecret) -> KeyVaultSecret: ...
    @abstractmethod
    async def delete_all(self, vault: str, name: str) -> None: ...
