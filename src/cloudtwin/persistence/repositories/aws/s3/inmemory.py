"""S3 — in-memory repository implementations."""

from __future__ import annotations

from typing import Optional

from cloudtwin.persistence.models.aws.s3 import S3Bucket, S3Object
from cloudtwin.persistence.repositories.aws.s3.repository import (
    S3BucketRepository,
    S3ObjectRepository,
)


class InMemoryS3BucketRepository(S3BucketRepository):
    def __init__(self):
        self._store: dict[str, S3Bucket] = {}
        self._next_id = 1

    async def get(self, name: str) -> Optional[S3Bucket]:
        return self._store.get(name)

    async def list_all(self) -> list[S3Bucket]:
        return list(self._store.values())

    async def save(self, bucket: S3Bucket) -> S3Bucket:
        if bucket.name not in self._store:
            bucket.id = self._next_id
            self._next_id += 1
        self._store[bucket.name] = bucket
        return bucket

    async def delete(self, name: str) -> None:
        self._store.pop(name, None)


class InMemoryS3ObjectRepository(S3ObjectRepository):
    def __init__(self):
        self._store: dict[tuple[int, str], S3Object] = {}

    async def get(self, bucket_id: int, key: str) -> Optional[S3Object]:
        return self._store.get((bucket_id, key))

    async def list_by_bucket(self, bucket_id: int, prefix: str = "") -> list[S3Object]:
        return [
            obj for (bid, k), obj in self._store.items()
            if bid == bucket_id and k.startswith(prefix)
        ]

    async def save(self, obj: S3Object) -> S3Object:
        self._store[(obj.bucket_id, obj.key)] = obj
        return obj

    async def delete(self, bucket_id: int, key: str) -> None:
        self._store.pop((bucket_id, key), None)
