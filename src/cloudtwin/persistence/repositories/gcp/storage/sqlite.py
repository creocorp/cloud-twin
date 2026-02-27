"""GCS — SQLite repository implementations."""

from __future__ import annotations

from datetime import datetime, timezone
from typing import Optional

from cloudtwin.persistence.models.gcp.storage import GcsBucket, GcsObject
from cloudtwin.persistence.repositories.gcp.storage.repository import (
    GcsBucketRepository,
    GcsObjectRepository,
)

DDL = """
CREATE TABLE IF NOT EXISTS gcs_buckets (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    project    TEXT    NOT NULL,
    name       TEXT    NOT NULL,
    location   TEXT,
    created_at TEXT    NOT NULL,
    UNIQUE(project, name)
);

CREATE TABLE IF NOT EXISTS gcs_objects (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    bucket_id      INTEGER NOT NULL REFERENCES gcs_buckets(id),
    name           TEXT    NOT NULL,
    content_type   TEXT,
    content_length INTEGER,
    data           BLOB,
    metadata       TEXT,   -- JSON string
    created_at     TEXT    NOT NULL,
    UNIQUE(bucket_id, name)
);
"""


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class SqliteGcsBucketRepository(GcsBucketRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> GcsBucket:
        return GcsBucket(
            id=row["id"], project=row["project"], name=row["name"],
            location=row["location"], created_at=row["created_at"],
        )

    async def get(self, project: str, name: str) -> Optional[GcsBucket]:
        async with self._db.conn.execute(
            "SELECT * FROM gcs_buckets WHERE project = ? AND name = ?", (project, name)
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def list_by_project(self, project: str) -> list[GcsBucket]:
        async with self._db.conn.execute(
            "SELECT * FROM gcs_buckets WHERE project = ?", (project,)
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def save(self, bucket: GcsBucket) -> GcsBucket:
        await self._db.conn.execute(
            "INSERT OR IGNORE INTO gcs_buckets (project, name, location, created_at) VALUES (?, ?, ?, ?)",
            (bucket.project, bucket.name, bucket.location, bucket.created_at or _now()),
        )
        await self._db.conn.commit()
        return await self.get(bucket.project, bucket.name)

    async def delete(self, project: str, name: str) -> None:
        await self._db.conn.execute(
            "DELETE FROM gcs_buckets WHERE project = ? AND name = ?", (project, name)
        )
        await self._db.conn.commit()


class SqliteGcsObjectRepository(GcsObjectRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> GcsObject:
        return GcsObject(
            id=row["id"], bucket_id=row["bucket_id"], name=row["name"],
            content_type=row["content_type"], content_length=row["content_length"],
            data=row["data"], metadata=row["metadata"], created_at=row["created_at"],
        )

    async def get(self, bucket_id: int, name: str) -> Optional[GcsObject]:
        async with self._db.conn.execute(
            "SELECT * FROM gcs_objects WHERE bucket_id = ? AND name = ?", (bucket_id, name)
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def list_by_bucket(self, bucket_id: int, prefix: str = "") -> list[GcsObject]:
        async with self._db.conn.execute(
            "SELECT * FROM gcs_objects WHERE bucket_id = ? AND name LIKE ?",
            (bucket_id, f"{prefix}%"),
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def save(self, obj: GcsObject) -> GcsObject:
        await self._db.conn.execute(
            """
            INSERT INTO gcs_objects
                (bucket_id, name, content_type, content_length, data, metadata, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(bucket_id, name) DO UPDATE SET
                content_type = excluded.content_type,
                content_length = excluded.content_length,
                data = excluded.data,
                metadata = excluded.metadata,
                created_at = excluded.created_at
            """,
            (obj.bucket_id, obj.name, obj.content_type, obj.content_length,
             obj.data, obj.metadata, obj.created_at or _now()),
        )
        await self._db.conn.commit()
        return obj

    async def delete(self, bucket_id: int, name: str) -> None:
        await self._db.conn.execute(
            "DELETE FROM gcs_objects WHERE bucket_id = ? AND name = ?", (bucket_id, name)
        )
        await self._db.conn.commit()
