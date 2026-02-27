"""AWS Lambda — SQLite repository implementations."""

from __future__ import annotations

from datetime import datetime, timezone
from typing import Optional

from cloudtwin.persistence.models.aws.lambda_ import LambdaFunction, LambdaInvocation
from cloudtwin.persistence.repositories.aws.lambda_.repository import (
    LambdaFunctionRepository,
    LambdaInvocationRepository,
)

DDL = """
CREATE TABLE IF NOT EXISTS lambda_functions (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT    NOT NULL UNIQUE,
    arn        TEXT    NOT NULL UNIQUE,
    runtime    TEXT    NOT NULL,
    handler    TEXT    NOT NULL,
    code       TEXT    NOT NULL DEFAULT '',
    created_at TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS lambda_invocations (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    function_name   TEXT    NOT NULL,
    invocation_id   TEXT    NOT NULL UNIQUE,
    payload         TEXT    NOT NULL,
    response        TEXT    NOT NULL,
    created_at      TEXT    NOT NULL
);
"""


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class SqliteLambdaFunctionRepository(LambdaFunctionRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> LambdaFunction:
        return LambdaFunction(
            id=row["id"], name=row["name"], arn=row["arn"],
            runtime=row["runtime"], handler=row["handler"],
            code=row["code"], created_at=row["created_at"],
        )

    async def get(self, name: str) -> Optional[LambdaFunction]:
        async with self._db.conn.execute("SELECT * FROM lambda_functions WHERE name = ?", (name,)) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def list_all(self) -> list[LambdaFunction]:
        async with self._db.conn.execute("SELECT * FROM lambda_functions ORDER BY id") as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def save(self, fn: LambdaFunction) -> LambdaFunction:
        await self._db.conn.execute(
            "INSERT OR REPLACE INTO lambda_functions (name, arn, runtime, handler, code, created_at) VALUES (?, ?, ?, ?, ?, ?)",
            (fn.name, fn.arn, fn.runtime, fn.handler, fn.code, fn.created_at or _now()),
        )
        await self._db.conn.commit()
        return await self.get(fn.name)

    async def delete(self, name: str) -> None:
        await self._db.conn.execute("DELETE FROM lambda_functions WHERE name = ?", (name,))
        await self._db.conn.commit()


class SqliteLambdaInvocationRepository(LambdaInvocationRepository):
    def __init__(self, db):
        self._db = db

    async def save(self, invocation: LambdaInvocation) -> LambdaInvocation:
        await self._db.conn.execute(
            "INSERT INTO lambda_invocations (function_name, invocation_id, payload, response, created_at) VALUES (?, ?, ?, ?, ?)",
            (invocation.function_name, invocation.invocation_id, invocation.payload, invocation.response, invocation.created_at or _now()),
        )
        await self._db.conn.commit()
        return invocation
