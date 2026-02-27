"""S3 — SQLite repository implementations."""

from __future__ import annotations

from datetime import datetime, timezone
from typing import Optional

from cloudtwin.persistence.models.aws.s3 import S3Bucket, S3Object
from cloudtwin.persistence.repositories.aws.s3.repository import (
    S3BucketRepository,
    S3ObjectRepository,
)

DDL = """
CREATE TABLE IF NOT EXISTS s3_buckets (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT    NOT NULL UNIQUE,
    created_at TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS s3_objects (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    bucket_id      INTEGER NOT NULL REFERENCES s3_buckets(id),
    key            TEXT    NOT NULL,
    content_type   TEXT,
    content_length INTEGER,
    data           BLOB,
    created_at     TEXT    NOT NULL,
    UNIQUE(bucket_id, key)
);
"""


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class SqliteS3BucketRepository(S3BucketRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> S3Bucket:
        return S3Bucket(id=row["id"], name=row["name"], created_at=row["created_at"])

    async def get(self, name: str) -> Optional[S3Bucket]:
        async with self._db.conn.execute("SELECT * FROM s3_buckets WHERE name = ?", (name,)) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def list_all(self) -> list[S3Bucket]:
        async with self._db.conn.execute("SELECT * FROM s3_buckets") as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def save(self, bucket: S3Bucket) -> S3Bucket:
        await self._db.conn.execute(
            "INSERT OR IGNORE INTO s3_buckets (name, created_at) VALUES (?, ?)",
            (bucket.name, bucket.created_at or _now()),
        )
        await self._db.conn.commit()
        return await self.get(bucket.name)

    async def delete(self, name: str) -> None:
        await self._db.conn.execute("DELETE FROM s3_buckets WHERE name = ?", (name,))
        await self._db.conn.commit()


class SqliteS3ObjectRepository(S3ObjectRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> S3Object:
        return S3Object(
            id=row["id"], bucket_id=row["bucket_id"], key=row["key"],
            content_type=row["content_type"], content_length=row["content_length"],
            data=row["data"], created_at=row["created_at"],
        )

    async def get(self, bucket_id: int, key: str) -> Optional[S3Object]:
        async with self._db.conn.execute(
            "SELECT * FROM s3_objects WHERE bucket_id = ? AND key = ?", (bucket_id, key)
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def list_by_bucket(self, bucket_id: int, prefix: str = "") -> list[S3Object]:
        async with self._db.conn.execute(
            "SELECT * FROM s3_objects WHERE bucket_id = ? AND key LIKE ?",
            (bucket_id, f"{prefix}%"),
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def save(self, obj: S3Object) -> S3Object:
        await self._db.conn.execute(
            """
            INSERT INTO s3_objects
                (bucket_id, key, content_type, content_length, data, created_at)
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(bucket_id, key) DO UPDATE SET
                content_type = excluded.content_type,
                content_length = excluded.content_length,
                data = excluded.data,
                created_at = excluded.created_at
            """,
            (obj.bucket_id, obj.key, obj.content_type, obj.content_length,
             obj.data, obj.created_at or _now()),
        )
        await self._db.conn.commit()
        return obj

    async def delete(self, bucket_id: int, key: str) -> None:
        await self._db.conn.execute(
            "DELETE FROM s3_objects WHERE bucket_id = ? AND key = ?", (bucket_id, key)
        )
        await self._db.conn.commit()
