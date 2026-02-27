"""GCP Cloud Functions — SQLite repository implementations."""

from __future__ import annotations

from datetime import datetime, timezone
from typing import Optional

from cloudtwin.persistence.models.gcp.cloudfunctions import CloudFunction, CloudFunctionInvocation
from cloudtwin.persistence.repositories.gcp.cloudfunctions.repository import (
    CloudFunctionInvocationRepository,
    CloudFunctionRepository,
)

DDL = """
CREATE TABLE IF NOT EXISTS gcp_functions (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    project     TEXT    NOT NULL,
    name        TEXT    NOT NULL,
    full_name   TEXT    NOT NULL UNIQUE,
    runtime     TEXT    NOT NULL,
    entry_point TEXT    NOT NULL,
    source_code TEXT    NOT NULL DEFAULT '',
    created_at  TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS gcp_function_invocations (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    function_full_name  TEXT    NOT NULL,
    invocation_id       TEXT    NOT NULL UNIQUE,
    payload             TEXT    NOT NULL,
    response            TEXT    NOT NULL,
    created_at          TEXT    NOT NULL
);
"""


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class SqliteCloudFunctionRepository(CloudFunctionRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> CloudFunction:
        return CloudFunction(
            id=row["id"], project=row["project"], name=row["name"],
            full_name=row["full_name"], runtime=row["runtime"],
            entry_point=row["entry_point"], source_code=row["source_code"],
            created_at=row["created_at"],
        )

    async def get(self, full_name: str) -> Optional[CloudFunction]:
        async with self._db.conn.execute(
            "SELECT * FROM gcp_functions WHERE full_name = ?", (full_name,)
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def list_by_project(self, project: str) -> list[CloudFunction]:
        async with self._db.conn.execute(
            "SELECT * FROM gcp_functions WHERE project = ? ORDER BY id", (project,)
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def save(self, fn: CloudFunction) -> CloudFunction:
        await self._db.conn.execute(
            "INSERT OR REPLACE INTO gcp_functions (project, name, full_name, runtime, entry_point, source_code, created_at) VALUES (?,?,?,?,?,?,?)",
            (fn.project, fn.name, fn.full_name, fn.runtime, fn.entry_point, fn.source_code, fn.created_at or _now()),
        )
        await self._db.conn.commit()
        return await self.get(fn.full_name)

    async def delete(self, full_name: str) -> None:
        await self._db.conn.execute("DELETE FROM gcp_functions WHERE full_name = ?", (full_name,))
        await self._db.conn.commit()


class SqliteCloudFunctionInvocationRepository(CloudFunctionInvocationRepository):
    def __init__(self, db):
        self._db = db

    async def save(self, inv: CloudFunctionInvocation) -> CloudFunctionInvocation:
        await self._db.conn.execute(
            "INSERT INTO gcp_function_invocations (function_full_name, invocation_id, payload, response, created_at) VALUES (?,?,?,?,?)",
            (inv.function_full_name, inv.invocation_id, inv.payload, inv.response, inv.created_at or _now()),
        )
        await self._db.conn.commit()
        return inv
