"""GCP Cloud Storage – pure business logic."""

from __future__ import annotations

import logging
from datetime import datetime, timezone

from cloudtwin.core.errors import NotFoundError
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.persistence.models.gcp import GcsBucket, GcsObject
from cloudtwin.persistence.repositories.gcp import (
    GcsBucketRepository,
    GcsObjectRepository,
)

log = logging.getLogger("cloudtwin.gcp.storage")


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class StorageService:
    def __init__(
        self,
        project: str,
        bucket_repo: GcsBucketRepository,
        object_repo: GcsObjectRepository,
        telemetry: TelemetryEngine,
    ):
        self._project = project
        self._buckets = bucket_repo
        self._objects = object_repo
        self._telemetry = telemetry

    # ------------------------------------------------------------------
    # Buckets
    # ------------------------------------------------------------------

    async def create_bucket(self, name: str, location: str = "US") -> GcsBucket:
        existing = await self._buckets.get(self._project, name)
        if existing:
            return existing
        bucket = GcsBucket(
            project=self._project, name=name, location=location, created_at=_now()
        )
        result = await self._buckets.save(bucket)
        await self._telemetry.emit("gcp", "storage", "create_bucket", {"bucket": name})
        return result

    async def get_bucket(self, name: str) -> GcsBucket:
        bucket = await self._buckets.get(self._project, name)
        if not bucket:
            raise NotFoundError(f"Bucket not found: {name}")
        return bucket

    async def list_buckets(self) -> list[GcsBucket]:
        return await self._buckets.list_by_project(self._project)

    async def delete_bucket(self, name: str) -> None:
        await self.get_bucket(name)
        await self._buckets.delete(self._project, name)
        await self._telemetry.emit("gcp", "storage", "delete_bucket", {"bucket": name})

    # ------------------------------------------------------------------
    # Objects
    # ------------------------------------------------------------------

    async def upload_object(
        self,
        bucket_name: str,
        object_name: str,
        data: bytes,
        content_type: str = "application/octet-stream",
    ) -> GcsObject:
        bucket = await self.get_bucket(bucket_name)
        import json

        obj = GcsObject(
            bucket_id=bucket.id,
            name=object_name,
            content_type=content_type,
            content_length=len(data),
            data=data,
            metadata=json.dumps({}),
            created_at=_now(),
        )
        result = await self._objects.save(obj)
        await self._telemetry.emit(
            "gcp",
            "storage",
            "upload_object",
            {"bucket": bucket_name, "object": object_name, "size": len(data)},
        )
        return result

    async def get_object(self, bucket_name: str, object_name: str) -> GcsObject:
        bucket = await self.get_bucket(bucket_name)
        obj = await self._objects.get(bucket.id, object_name)
        if not obj:
            raise NotFoundError(f"Object not found: {bucket_name}/{object_name}")
        return obj

    async def list_objects(self, bucket_name: str, prefix: str = "") -> list[GcsObject]:
        bucket = await self.get_bucket(bucket_name)
        return await self._objects.list_by_bucket(bucket.id, prefix=prefix)

    async def delete_object(self, bucket_name: str, object_name: str) -> None:
        bucket = await self.get_bucket(bucket_name)
        obj = await self._objects.get(bucket.id, object_name)
        if not obj:
            raise NotFoundError(f"Object not found: {bucket_name}/{object_name}")
        await self._objects.delete(bucket.id, object_name)
        await self._telemetry.emit(
            "gcp",
            "storage",
            "delete_object",
            {"bucket": bucket_name, "object": object_name},
        )
