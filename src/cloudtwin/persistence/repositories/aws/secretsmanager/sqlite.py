"""AWS Secrets Manager — SQLite repository implementations."""

from __future__ import annotations

from datetime import datetime, timezone
from typing import Optional

from cloudtwin.persistence.models.aws.secretsmanager import Secret, SecretVersion
from cloudtwin.persistence.repositories.aws.secretsmanager.repository import (
    SecretRepository,
    SecretVersionRepository,
)

DDL = """
CREATE TABLE IF NOT EXISTS sm_secrets (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT    NOT NULL UNIQUE,
    arn        TEXT    NOT NULL UNIQUE,
    created_at TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS sm_secret_versions (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    secret_name   TEXT    NOT NULL,
    version_id    TEXT    NOT NULL,
    secret_string TEXT,
    secret_binary BLOB,
    created_at    TEXT    NOT NULL,
    UNIQUE(secret_name, version_id)
);
"""


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class SqliteSecretRepository(SecretRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> Secret:
        return Secret(id=row["id"], name=row["name"], arn=row["arn"], created_at=row["created_at"])

    async def get(self, name: str) -> Optional[Secret]:
        async with self._db.conn.execute("SELECT * FROM sm_secrets WHERE name = ?", (name,)) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def list_all(self) -> list[Secret]:
        async with self._db.conn.execute("SELECT * FROM sm_secrets ORDER BY id") as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def save(self, secret: Secret) -> Secret:
        await self._db.conn.execute(
            "INSERT OR REPLACE INTO sm_secrets (name, arn, created_at) VALUES (?, ?, ?)",
            (secret.name, secret.arn, secret.created_at or _now()),
        )
        await self._db.conn.commit()
        return await self.get(secret.name)

    async def delete(self, name: str) -> None:
        await self._db.conn.execute("DELETE FROM sm_secrets WHERE name = ?", (name,))
        await self._db.conn.commit()


class SqliteSecretVersionRepository(SecretVersionRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> SecretVersion:
        return SecretVersion(
            id=row["id"],
            secret_name=row["secret_name"],
            version_id=row["version_id"],
            secret_string=row["secret_string"],
            secret_binary=row["secret_binary"],
            created_at=row["created_at"],
        )

    async def save(self, version: SecretVersion) -> SecretVersion:
        await self._db.conn.execute(
            "INSERT OR REPLACE INTO sm_secret_versions (secret_name, version_id, secret_string, secret_binary, created_at) VALUES (?, ?, ?, ?, ?)",
            (version.secret_name, version.version_id, version.secret_string, version.secret_binary, version.created_at or _now()),
        )
        await self._db.conn.commit()
        return version

    async def get_latest(self, secret_name: str) -> Optional[SecretVersion]:
        async with self._db.conn.execute(
            "SELECT * FROM sm_secret_versions WHERE secret_name = ? ORDER BY id DESC LIMIT 1", (secret_name,)
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def get_by_version_id(self, secret_name: str, version_id: str) -> Optional[SecretVersion]:
        async with self._db.conn.execute(
            "SELECT * FROM sm_secret_versions WHERE secret_name = ? AND version_id = ?", (secret_name, version_id)
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def delete_all(self, secret_name: str) -> None:
        await self._db.conn.execute("DELETE FROM sm_secret_versions WHERE secret_name = ?", (secret_name,))
        await self._db.conn.commit()
