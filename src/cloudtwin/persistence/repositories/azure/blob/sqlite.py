"""Azure Blob Storage — SQLite repository implementations."""

from __future__ import annotations

from datetime import datetime, timezone
from typing import Optional

from cloudtwin.persistence.models.azure.blob import AzureBlob, AzureContainer
from cloudtwin.persistence.repositories.azure.blob.repository import (
    AzureBlobRepository,
    AzureContainerRepository,
)

DDL = """
CREATE TABLE IF NOT EXISTS azure_containers (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    account    TEXT    NOT NULL,
    name       TEXT    NOT NULL,
    created_at TEXT    NOT NULL,
    UNIQUE(account, name)
);

CREATE TABLE IF NOT EXISTS azure_blobs (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    container_id   INTEGER NOT NULL REFERENCES azure_containers(id),
    name           TEXT    NOT NULL,
    content_type   TEXT,
    content_length INTEGER,
    data           BLOB,
    metadata       TEXT,   -- JSON string
    created_at     TEXT    NOT NULL,
    UNIQUE(container_id, name)
);
"""


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class SqliteAzureContainerRepository(AzureContainerRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> AzureContainer:
        return AzureContainer(
            id=row["id"],
            account=row["account"],
            name=row["name"],
            created_at=row["created_at"],
        )

    async def get(self, account: str, name: str) -> Optional[AzureContainer]:
        async with self._db.conn.execute(
            "SELECT * FROM azure_containers WHERE account = ? AND name = ?",
            (account, name),
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def list_by_account(self, account: str) -> list[AzureContainer]:
        async with self._db.conn.execute(
            "SELECT * FROM azure_containers WHERE account = ?", (account,)
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def save(self, container: AzureContainer) -> AzureContainer:
        await self._db.conn.execute(
            "INSERT OR IGNORE INTO azure_containers (account, name, created_at) VALUES (?, ?, ?)",
            (container.account, container.name, container.created_at or _now()),
        )
        await self._db.conn.commit()
        return await self.get(container.account, container.name)

    async def delete(self, account: str, name: str) -> None:
        await self._db.conn.execute(
            "DELETE FROM azure_containers WHERE account = ? AND name = ?",
            (account, name),
        )
        await self._db.conn.commit()


class SqliteAzureBlobRepository(AzureBlobRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> AzureBlob:
        return AzureBlob(
            id=row["id"],
            container_id=row["container_id"],
            name=row["name"],
            content_type=row["content_type"],
            content_length=row["content_length"],
            data=row["data"],
            metadata=row["metadata"],
            created_at=row["created_at"],
        )

    async def get(self, container_id: int, name: str) -> Optional[AzureBlob]:
        async with self._db.conn.execute(
            "SELECT * FROM azure_blobs WHERE container_id = ? AND name = ?",
            (container_id, name),
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def list_by_container(
        self, container_id: int, prefix: str = ""
    ) -> list[AzureBlob]:
        async with self._db.conn.execute(
            "SELECT * FROM azure_blobs WHERE container_id = ? AND name LIKE ?",
            (container_id, f"{prefix}%"),
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def save(self, blob: AzureBlob) -> AzureBlob:
        await self._db.conn.execute(
            """
            INSERT INTO azure_blobs
                (container_id, name, content_type, content_length, data, metadata, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(container_id, name) DO UPDATE SET
                content_type = excluded.content_type,
                content_length = excluded.content_length,
                data = excluded.data,
                metadata = excluded.metadata,
                created_at = excluded.created_at
            """,
            (
                blob.container_id,
                blob.name,
                blob.content_type,
                blob.content_length,
                blob.data,
                blob.metadata,
                blob.created_at or _now(),
            ),
        )
        await self._db.conn.commit()
        return blob

    async def delete(self, container_id: int, name: str) -> None:
        await self._db.conn.execute(
            "DELETE FROM azure_blobs WHERE container_id = ? AND name = ?",
            (container_id, name),
        )
        await self._db.conn.commit()
