"""Azure Functions — SQLite repository implementations."""

from __future__ import annotations

from datetime import datetime, timezone
from typing import Optional

from cloudtwin.persistence.models.azure.functions import (
    AzureFunction,
    AzureFunctionInvocation,
)
from cloudtwin.persistence.repositories.azure.functions.repository import (
    AzureFunctionInvocationRepository,
    AzureFunctionRepository,
)

DDL = """
CREATE TABLE IF NOT EXISTS azure_functions (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    app        TEXT    NOT NULL,
    name       TEXT    NOT NULL,
    code       TEXT    NOT NULL DEFAULT '',
    created_at TEXT    NOT NULL,
    UNIQUE(app, name)
);

CREATE TABLE IF NOT EXISTS azure_function_invocations (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    app             TEXT    NOT NULL,
    function_name   TEXT    NOT NULL,
    invocation_id   TEXT    NOT NULL UNIQUE,
    payload         TEXT    NOT NULL,
    response        TEXT    NOT NULL,
    created_at      TEXT    NOT NULL
);
"""


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class SqliteAzureFunctionRepository(AzureFunctionRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> AzureFunction:
        return AzureFunction(
            id=row["id"],
            app=row["app"],
            name=row["name"],
            code=row["code"],
            created_at=row["created_at"],
        )

    async def get(self, app: str, name: str) -> Optional[AzureFunction]:
        async with self._db.conn.execute(
            "SELECT * FROM azure_functions WHERE app = ? AND name = ?", (app, name)
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def list_by_app(self, app: str) -> list[AzureFunction]:
        async with self._db.conn.execute(
            "SELECT * FROM azure_functions WHERE app = ? ORDER BY id", (app,)
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def save(self, fn: AzureFunction) -> AzureFunction:
        await self._db.conn.execute(
            "INSERT OR REPLACE INTO azure_functions (app, name, code, created_at) VALUES (?, ?, ?, ?)",
            (fn.app, fn.name, fn.code, fn.created_at or _now()),
        )
        await self._db.conn.commit()
        return await self.get(fn.app, fn.name)

    async def delete(self, app: str, name: str) -> None:
        await self._db.conn.execute(
            "DELETE FROM azure_functions WHERE app = ? AND name = ?", (app, name)
        )
        await self._db.conn.commit()


class SqliteAzureFunctionInvocationRepository(AzureFunctionInvocationRepository):
    def __init__(self, db):
        self._db = db

    async def save(self, inv: AzureFunctionInvocation) -> AzureFunctionInvocation:
        await self._db.conn.execute(
            "INSERT INTO azure_function_invocations (app, function_name, invocation_id, payload, response, created_at) VALUES (?, ?, ?, ?, ?, ?)",
            (
                inv.app,
                inv.function_name,
                inv.invocation_id,
                inv.payload,
                inv.response,
                inv.created_at or _now(),
            ),
        )
        await self._db.conn.commit()
        return inv
