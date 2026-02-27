"""Azure Key Vault — SQLite repository implementations."""

from __future__ import annotations

from datetime import datetime, timezone
from typing import Optional

from cloudtwin.persistence.models.azure.keyvault import KeyVaultSecret
from cloudtwin.persistence.repositories.azure.keyvault.repository import KeyVaultSecretRepository

DDL = """
CREATE TABLE IF NOT EXISTS kv_secrets (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    vault      TEXT    NOT NULL,
    name       TEXT    NOT NULL,
    value      TEXT    NOT NULL,
    version    TEXT    NOT NULL,
    created_at TEXT    NOT NULL
);
"""


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class SqliteKeyVaultSecretRepository(KeyVaultSecretRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> KeyVaultSecret:
        return KeyVaultSecret(id=row["id"], vault=row["vault"], name=row["name"],
                              value=row["value"], version=row["version"], created_at=row["created_at"])

    async def get_latest(self, vault: str, name: str) -> Optional[KeyVaultSecret]:
        async with self._db.conn.execute(
            "SELECT * FROM kv_secrets WHERE vault = ? AND name = ? ORDER BY id DESC LIMIT 1", (vault, name)
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def get_version(self, vault: str, name: str, version: str) -> Optional[KeyVaultSecret]:
        async with self._db.conn.execute(
            "SELECT * FROM kv_secrets WHERE vault = ? AND name = ? AND version = ?", (vault, name, version)
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def list_by_vault(self, vault: str) -> list[KeyVaultSecret]:
        async with self._db.conn.execute(
            "SELECT * FROM (SELECT * FROM kv_secrets WHERE vault = ? ORDER BY id DESC) GROUP BY name", (vault,)
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def save(self, secret: KeyVaultSecret) -> KeyVaultSecret:
        await self._db.conn.execute(
            "INSERT INTO kv_secrets (vault, name, value, version, created_at) VALUES (?, ?, ?, ?, ?)",
            (secret.vault, secret.name, secret.value, secret.version, secret.created_at or _now()),
        )
        await self._db.conn.commit()
        return secret

    async def delete_all(self, vault: str, name: str) -> None:
        await self._db.conn.execute("DELETE FROM kv_secrets WHERE vault = ? AND name = ?", (vault, name))
        await self._db.conn.commit()
