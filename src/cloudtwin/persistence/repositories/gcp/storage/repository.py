"""GCS — abstract repository interfaces."""

from __future__ import annotations

from abc import ABC, abstractmethod
from typing import Optional

from cloudtwin.persistence.models.gcp.storage import GcsBucket, GcsObject


class GcsBucketRepository(ABC):
    @abstractmethod
    async def get(self, project: str, name: str) -> Optional[GcsBucket]: ...
    @abstractmethod
    async def list_by_project(self, project: str) -> list[GcsBucket]: ...
    @abstractmethod
    async def save(self, bucket: GcsBucket) -> GcsBucket: ...
    @abstractmethod
    async def delete(self, project: str, name: str) -> None: ...


class GcsObjectRepository(ABC):
    @abstractmethod
    async def get(self, bucket_id: int, name: str) -> Optional[GcsObject]: ...
    @abstractmethod
    async def list_by_bucket(
        self, bucket_id: int, prefix: str = ""
    ) -> list[GcsObject]: ...
    @abstractmethod
    async def save(self, obj: GcsObject) -> GcsObject: ...
    @abstractmethod
    async def delete(self, bucket_id: int, name: str) -> None: ...
