"""Azure Key Vault — in-memory repository implementations."""

from __future__ import annotations

from typing import Optional

from cloudtwin.persistence.models.azure.keyvault import KeyVaultSecret
from cloudtwin.persistence.repositories.azure.keyvault.repository import (
    KeyVaultSecretRepository,
)


class InMemoryKeyVaultSecretRepository(KeyVaultSecretRepository):
    def __init__(self):
        self._store: list[KeyVaultSecret] = []
        self._next_id = 1

    async def get_latest(self, vault: str, name: str) -> Optional[KeyVaultSecret]:
        matches = [s for s in self._store if s.vault == vault and s.name == name]
        return matches[-1] if matches else None

    async def get_version(
        self, vault: str, name: str, version: str
    ) -> Optional[KeyVaultSecret]:
        return next(
            (
                s
                for s in self._store
                if s.vault == vault and s.name == name and s.version == version
            ),
            None,
        )

    async def list_by_vault(self, vault: str) -> list[KeyVaultSecret]:
        seen: dict[str, KeyVaultSecret] = {}
        for s in self._store:
            if s.vault == vault:
                seen[s.name] = s
        return list(seen.values())

    async def save(self, secret: KeyVaultSecret) -> KeyVaultSecret:
        secret.id = self._next_id
        self._next_id += 1
        self._store.append(secret)
        return secret

    async def delete_all(self, vault: str, name: str) -> None:
        self._store = [
            s for s in self._store if not (s.vault == vault and s.name == name)
        ]
