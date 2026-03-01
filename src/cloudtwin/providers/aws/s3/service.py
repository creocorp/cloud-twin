"""
S3 domain service.

Business logic for S3 operations – no HTTP knowledge.
"""

from __future__ import annotations

from datetime import datetime, timezone

from cloudtwin.core.errors import NotFoundError
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.persistence.models import S3Bucket, S3Object
from cloudtwin.persistence.repositories import S3BucketRepository, S3ObjectRepository


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class S3Service:
    def __init__(
        self,
        bucket_repo: S3BucketRepository,
        object_repo: S3ObjectRepository,
        telemetry: TelemetryEngine,
    ):
        self._bucket_repo = bucket_repo
        self._object_repo = object_repo
        self._telemetry = telemetry

    # ------------------------------------------------------------------
    # Buckets
    # ------------------------------------------------------------------

    async def create_bucket(self, name: str) -> S3Bucket:
        bucket = S3Bucket(id=None, name=name, created_at=_now())
        result = await self._bucket_repo.save(bucket)
        await self._telemetry.emit("aws", "s3", "create_bucket", {"bucket": name})
        return result

    async def list_buckets(self) -> list[S3Bucket]:
        return await self._bucket_repo.list_all()

    async def _require_bucket(self, name: str) -> S3Bucket:
        bucket = await self._bucket_repo.get(name)
        if bucket is None:
            raise NotFoundError(f"Bucket not found: {name}")
        return bucket

    # ------------------------------------------------------------------
    # Objects
    # ------------------------------------------------------------------

    async def put_object(
        self,
        bucket_name: str,
        key: str,
        data: bytes,
        content_type: str | None = None,
    ) -> S3Object:
        bucket = await self._require_bucket(bucket_name)
        obj = S3Object(
            id=None,
            bucket_id=bucket.id,
            key=key,
            content_type=content_type or "application/octet-stream",
            content_length=len(data),
            data=data,
            created_at=_now(),
        )
        result = await self._object_repo.save(obj)
        await self._telemetry.emit(
            "aws",
            "s3",
            "put_object",
            {
                "bucket": bucket_name,
                "key": key,
                "size": len(data),
            },
        )
        return result

    async def get_object(self, bucket_name: str, key: str) -> S3Object:
        bucket = await self._require_bucket(bucket_name)
        obj = await self._object_repo.get(bucket.id, key)
        if obj is None:
            raise NotFoundError(f"Object not found: s3://{bucket_name}/{key}")
        await self._telemetry.emit(
            "aws",
            "s3",
            "get_object",
            {
                "bucket": bucket_name,
                "key": key,
            },
        )
        return obj

    async def delete_object(self, bucket_name: str, key: str) -> None:
        bucket = await self._require_bucket(bucket_name)
        await self._object_repo.delete(bucket.id, key)
        await self._telemetry.emit(
            "aws",
            "s3",
            "delete_object",
            {
                "bucket": bucket_name,
                "key": key,
            },
        )

    async def list_objects_v2(
        self, bucket_name: str, prefix: str = "", max_keys: int = 1000
    ) -> list[S3Object]:
        bucket = await self._require_bucket(bucket_name)
        objects = await self._object_repo.list_by_bucket(bucket.id, prefix)
        return objects[:max_keys]
