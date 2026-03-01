"""AWS DynamoDB — SQLite repository implementations."""

from __future__ import annotations

from datetime import datetime, timezone
from typing import Optional

from cloudtwin.persistence.models.aws.dynamodb import DynamoItem, DynamoTable
from cloudtwin.persistence.repositories.aws.dynamodb.repository import (
    DynamoItemRepository,
    DynamoTableRepository,
)

DDL = """
CREATE TABLE IF NOT EXISTS dynamo_tables (
    id                    INTEGER PRIMARY KEY AUTOINCREMENT,
    name                  TEXT    NOT NULL UNIQUE,
    key_schema            TEXT    NOT NULL,
    attribute_definitions TEXT    NOT NULL,
    created_at            TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS dynamo_items (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    table_name TEXT    NOT NULL,
    pk         TEXT    NOT NULL,
    sk         TEXT    NOT NULL DEFAULT '',
    item       TEXT    NOT NULL,
    created_at TEXT    NOT NULL,
    UNIQUE(table_name, pk, sk)
);
"""


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class SqliteDynamoTableRepository(DynamoTableRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> DynamoTable:
        return DynamoTable(
            id=row["id"],
            name=row["name"],
            key_schema=row["key_schema"],
            attribute_definitions=row["attribute_definitions"],
            created_at=row["created_at"],
        )

    async def get(self, name: str) -> Optional[DynamoTable]:
        async with self._db.conn.execute(
            "SELECT * FROM dynamo_tables WHERE name = ?", (name,)
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def list_all(self) -> list[DynamoTable]:
        async with self._db.conn.execute(
            "SELECT * FROM dynamo_tables ORDER BY id"
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def save(self, table: DynamoTable) -> DynamoTable:
        await self._db.conn.execute(
            "INSERT OR IGNORE INTO dynamo_tables (name, key_schema, attribute_definitions, created_at) VALUES (?, ?, ?, ?)",
            (
                table.name,
                table.key_schema,
                table.attribute_definitions,
                table.created_at or _now(),
            ),
        )
        await self._db.conn.commit()
        return await self.get(table.name)

    async def delete(self, name: str) -> None:
        await self._db.conn.execute("DELETE FROM dynamo_tables WHERE name = ?", (name,))
        await self._db.conn.commit()


class SqliteDynamoItemRepository(DynamoItemRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> DynamoItem:
        return DynamoItem(
            id=row["id"],
            table_name=row["table_name"],
            pk=row["pk"],
            sk=row["sk"],
            item=row["item"],
            created_at=row["created_at"],
        )

    async def put(self, item: DynamoItem) -> DynamoItem:
        await self._db.conn.execute(
            "INSERT OR REPLACE INTO dynamo_items (table_name, pk, sk, item, created_at) VALUES (?, ?, ?, ?, ?)",
            (item.table_name, item.pk, item.sk, item.item, item.created_at or _now()),
        )
        await self._db.conn.commit()
        return item

    async def get(self, table_name: str, pk: str, sk: str) -> Optional[DynamoItem]:
        async with self._db.conn.execute(
            "SELECT * FROM dynamo_items WHERE table_name = ? AND pk = ? AND sk = ?",
            (table_name, pk, sk),
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def delete(self, table_name: str, pk: str, sk: str) -> None:
        await self._db.conn.execute(
            "DELETE FROM dynamo_items WHERE table_name = ? AND pk = ? AND sk = ?",
            (table_name, pk, sk),
        )
        await self._db.conn.commit()

    async def scan(self, table_name: str) -> list[DynamoItem]:
        async with self._db.conn.execute(
            "SELECT * FROM dynamo_items WHERE table_name = ?", (table_name,)
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def query(self, table_name: str, pk: str) -> list[DynamoItem]:
        async with self._db.conn.execute(
            "SELECT * FROM dynamo_items WHERE table_name = ? AND pk = ?",
            (table_name, pk),
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def delete_all(self, table_name: str) -> None:
        await self._db.conn.execute(
            "DELETE FROM dynamo_items WHERE table_name = ?", (table_name,)
        )
        await self._db.conn.commit()
