"""GCP Secret Manager — SQLite repository implementations."""

from __future__ import annotations

from datetime import datetime, timezone
from typing import Optional

from cloudtwin.persistence.models.gcp.secretmanager import GcpSecret, GcpSecretVersion
from cloudtwin.persistence.repositories.gcp.secretmanager.repository import (
    GcpSecretRepository,
    GcpSecretVersionRepository,
)

DDL = """
CREATE TABLE IF NOT EXISTS gcp_secrets (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    project    TEXT    NOT NULL,
    name       TEXT    NOT NULL,
    full_name  TEXT    NOT NULL UNIQUE,
    created_at TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS gcp_secret_versions (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    secret_full_name TEXT    NOT NULL,
    version_id       TEXT    NOT NULL,
    payload          TEXT    NOT NULL DEFAULT '',
    state            TEXT    NOT NULL DEFAULT 'enabled',
    created_at       TEXT    NOT NULL,
    UNIQUE(secret_full_name, version_id)
);
"""


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class SqliteGcpSecretRepository(GcpSecretRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> GcpSecret:
        return GcpSecret(
            id=row["id"],
            project=row["project"],
            name=row["name"],
            full_name=row["full_name"],
            created_at=row["created_at"],
        )

    async def get(self, full_name: str) -> Optional[GcpSecret]:
        async with self._db.conn.execute(
            "SELECT * FROM gcp_secrets WHERE full_name = ?", (full_name,)
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def list_by_project(self, project: str) -> list[GcpSecret]:
        async with self._db.conn.execute(
            "SELECT * FROM gcp_secrets WHERE project = ? ORDER BY id", (project,)
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def save(self, secret: GcpSecret) -> GcpSecret:
        await self._db.conn.execute(
            "INSERT OR IGNORE INTO gcp_secrets (project, name, full_name, created_at) VALUES (?,?,?,?)",
            (
                secret.project,
                secret.name,
                secret.full_name,
                secret.created_at or _now(),
            ),
        )
        await self._db.conn.commit()
        return await self.get(secret.full_name)

    async def delete(self, full_name: str) -> None:
        await self._db.conn.execute(
            "DELETE FROM gcp_secrets WHERE full_name = ?", (full_name,)
        )
        await self._db.conn.commit()


class SqliteGcpSecretVersionRepository(GcpSecretVersionRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> GcpSecretVersion:
        return GcpSecretVersion(
            id=row["id"],
            secret_full_name=row["secret_full_name"],
            version_id=row["version_id"],
            payload=row["payload"],
            state=row["state"],
            created_at=row["created_at"],
        )

    async def save(self, version: GcpSecretVersion) -> GcpSecretVersion:
        await self._db.conn.execute(
            "INSERT OR REPLACE INTO gcp_secret_versions (secret_full_name, version_id, payload, state, created_at) VALUES (?,?,?,?,?)",
            (
                version.secret_full_name,
                version.version_id,
                version.payload,
                version.state,
                version.created_at or _now(),
            ),
        )
        await self._db.conn.commit()
        return version

    async def get_latest(self, secret_full_name: str) -> Optional[GcpSecretVersion]:
        async with self._db.conn.execute(
            "SELECT * FROM gcp_secret_versions WHERE secret_full_name = ? AND state = 'enabled' ORDER BY id DESC LIMIT 1",
            (secret_full_name,),
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def get_by_version_id(
        self, secret_full_name: str, version_id: str
    ) -> Optional[GcpSecretVersion]:
        async with self._db.conn.execute(
            "SELECT * FROM gcp_secret_versions WHERE secret_full_name = ? AND version_id = ?",
            (secret_full_name, version_id),
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def list_by_secret(self, secret_full_name: str) -> list[GcpSecretVersion]:
        async with self._db.conn.execute(
            "SELECT * FROM gcp_secret_versions WHERE secret_full_name = ? ORDER BY id",
            (secret_full_name,),
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def delete_all(self, secret_full_name: str) -> None:
        await self._db.conn.execute(
            "DELETE FROM gcp_secret_versions WHERE secret_full_name = ?",
            (secret_full_name,),
        )
        await self._db.conn.commit()
