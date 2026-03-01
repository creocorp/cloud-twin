"""S3 — abstract repository interfaces."""

from __future__ import annotations

from abc import ABC, abstractmethod
from typing import Optional

from cloudtwin.persistence.models.aws.s3 import S3Bucket, S3Object


class S3BucketRepository(ABC):
    @abstractmethod
    async def get(self, name: str) -> Optional[S3Bucket]: ...
    @abstractmethod
    async def list_all(self) -> list[S3Bucket]: ...
    @abstractmethod
    async def save(self, bucket: S3Bucket) -> S3Bucket: ...
    @abstractmethod
    async def delete(self, name: str) -> None: ...


class S3ObjectRepository(ABC):
    @abstractmethod
    async def get(self, bucket_id: int, key: str) -> Optional[S3Object]: ...
    @abstractmethod
    async def list_by_bucket(
        self, bucket_id: int, prefix: str = ""
    ) -> list[S3Object]: ...
    @abstractmethod
    async def save(self, obj: S3Object) -> S3Object: ...
    @abstractmethod
    async def delete(self, bucket_id: int, key: str) -> None: ...
