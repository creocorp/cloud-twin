"""AWS Secrets Manager — in-memory repository implementations."""

from __future__ import annotations

from typing import Optional

from cloudtwin.persistence.models.aws.secretsmanager import Secret, SecretVersion
from cloudtwin.persistence.repositories.aws.secretsmanager.repository import (
    SecretRepository,
    SecretVersionRepository,
)


class InMemorySecretRepository(SecretRepository):
    def __init__(self):
        self._store: dict[str, Secret] = {}
        self._next_id = 1

    async def get(self, name: str) -> Optional[Secret]:
        return self._store.get(name)

    async def list_all(self) -> list[Secret]:
        return list(self._store.values())

    async def save(self, secret: Secret) -> Secret:
        if secret.name not in self._store:
            secret.id = self._next_id
            self._next_id += 1
        self._store[secret.name] = secret
        return secret

    async def delete(self, name: str) -> None:
        self._store.pop(name, None)


class InMemorySecretVersionRepository(SecretVersionRepository):
    def __init__(self):
        self._store: list[SecretVersion] = []
        self._next_id = 1

    async def save(self, version: SecretVersion) -> SecretVersion:
        version.id = self._next_id
        self._next_id += 1
        self._store.append(version)
        return version

    async def get_latest(self, secret_name: str) -> Optional[SecretVersion]:
        matches = [v for v in self._store if v.secret_name == secret_name]
        return matches[-1] if matches else None

    async def get_by_version_id(
        self, secret_name: str, version_id: str
    ) -> Optional[SecretVersion]:
        return next(
            (
                v
                for v in self._store
                if v.secret_name == secret_name and v.version_id == version_id
            ),
            None,
        )

    async def delete_all(self, secret_name: str) -> None:
        self._store = [v for v in self._store if v.secret_name != secret_name]
