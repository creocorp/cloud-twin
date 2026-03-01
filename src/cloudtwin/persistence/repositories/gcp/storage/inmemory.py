"""GCS — in-memory repository implementations."""

from __future__ import annotations

from typing import Optional

from cloudtwin.persistence.models.gcp.storage import GcsBucket, GcsObject
from cloudtwin.persistence.repositories.gcp.storage.repository import (
    GcsBucketRepository,
    GcsObjectRepository,
)


class InMemoryGcsBucketRepository(GcsBucketRepository):
    def __init__(self):
        self._store: dict[tuple[str, str], GcsBucket] = {}
        self._next_id = 1

    async def get(self, project: str, name: str) -> Optional[GcsBucket]:
        return self._store.get((project, name))

    async def list_by_project(self, project: str) -> list[GcsBucket]:
        return [b for (p, _), b in self._store.items() if p == project]

    async def save(self, bucket: GcsBucket) -> GcsBucket:
        key = (bucket.project, bucket.name)
        if key not in self._store:
            bucket.id = self._next_id
            self._next_id += 1
        self._store[key] = bucket
        return bucket

    async def delete(self, project: str, name: str) -> None:
        self._store.pop((project, name), None)


class InMemoryGcsObjectRepository(GcsObjectRepository):
    def __init__(self):
        self._store: dict[tuple[int, str], GcsObject] = {}

    async def get(self, bucket_id: int, name: str) -> Optional[GcsObject]:
        return self._store.get((bucket_id, name))

    async def list_by_bucket(self, bucket_id: int, prefix: str = "") -> list[GcsObject]:
        return [
            o
            for (bid, k), o in self._store.items()
            if bid == bucket_id and k.startswith(prefix)
        ]

    async def save(self, obj: GcsObject) -> GcsObject:
        self._store[(obj.bucket_id, obj.name)] = obj
        return obj

    async def delete(self, bucket_id: int, name: str) -> None:
        self._store.pop((bucket_id, name), None)
