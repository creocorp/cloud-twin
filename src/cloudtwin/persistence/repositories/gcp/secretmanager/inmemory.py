"""GCP Secret Manager — in-memory repository implementations."""

from __future__ import annotations

from typing import Optional

from cloudtwin.persistence.models.gcp.secretmanager import GcpSecret, GcpSecretVersion
from cloudtwin.persistence.repositories.gcp.secretmanager.repository import (
    GcpSecretRepository,
    GcpSecretVersionRepository,
)


class InMemoryGcpSecretRepository(GcpSecretRepository):
    def __init__(self):
        self._store: dict[str, GcpSecret] = {}
        self._next_id = 1

    async def get(self, full_name: str) -> Optional[GcpSecret]:
        return self._store.get(full_name)

    async def list_by_project(self, project: str) -> list[GcpSecret]:
        return [s for s in self._store.values() if s.project == project]

    async def save(self, secret: GcpSecret) -> GcpSecret:
        if secret.full_name not in self._store:
            secret.id = self._next_id
            self._next_id += 1
        self._store[secret.full_name] = secret
        return secret

    async def delete(self, full_name: str) -> None:
        self._store.pop(full_name, None)


class InMemoryGcpSecretVersionRepository(GcpSecretVersionRepository):
    def __init__(self):
        self._store: list[GcpSecretVersion] = []
        self._next_id = 1

    async def save(self, version: GcpSecretVersion) -> GcpSecretVersion:
        version.id = self._next_id
        self._next_id += 1
        self._store.append(version)
        return version

    async def get_latest(self, secret_full_name: str) -> Optional[GcpSecretVersion]:
        matches = [v for v in self._store if v.secret_full_name == secret_full_name and v.state == "enabled"]
        return matches[-1] if matches else None

    async def get_by_version_id(self, secret_full_name: str, version_id: str) -> Optional[GcpSecretVersion]:
        return next(
            (v for v in self._store if v.secret_full_name == secret_full_name and v.version_id == version_id),
            None,
        )

    async def list_by_secret(self, secret_full_name: str) -> list[GcpSecretVersion]:
        return [v for v in self._store if v.secret_full_name == secret_full_name]

    async def delete_all(self, secret_full_name: str) -> None:
        self._store = [v for v in self._store if v.secret_full_name != secret_full_name]
