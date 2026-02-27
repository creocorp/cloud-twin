"""AWS DynamoDB — in-memory repository implementations."""

from __future__ import annotations

from typing import Optional

from cloudtwin.persistence.models.aws.dynamodb import DynamoItem, DynamoTable
from cloudtwin.persistence.repositories.aws.dynamodb.repository import (
    DynamoItemRepository,
    DynamoTableRepository,
)


class InMemoryDynamoTableRepository(DynamoTableRepository):
    def __init__(self):
        self._store: dict[str, DynamoTable] = {}
        self._next_id = 1

    async def get(self, name: str) -> Optional[DynamoTable]:
        return self._store.get(name)

    async def list_all(self) -> list[DynamoTable]:
        return list(self._store.values())

    async def save(self, table: DynamoTable) -> DynamoTable:
        if table.name not in self._store:
            table.id = self._next_id
            self._next_id += 1
        self._store[table.name] = table
        return table

    async def delete(self, name: str) -> None:
        self._store.pop(name, None)


class InMemoryDynamoItemRepository(DynamoItemRepository):
    def __init__(self):
        self._store: dict[tuple[str, str, str], DynamoItem] = {}
        self._next_id = 1

    def _key(self, table_name: str, pk: str, sk: str) -> tuple[str, str, str]:
        return (table_name, pk, sk)

    async def put(self, item: DynamoItem) -> DynamoItem:
        k = self._key(item.table_name, item.pk, item.sk)
        if k not in self._store:
            item.id = self._next_id
            self._next_id += 1
        self._store[k] = item
        return item

    async def get(self, table_name: str, pk: str, sk: str) -> Optional[DynamoItem]:
        return self._store.get(self._key(table_name, pk, sk))

    async def delete(self, table_name: str, pk: str, sk: str) -> None:
        self._store.pop(self._key(table_name, pk, sk), None)

    async def scan(self, table_name: str) -> list[DynamoItem]:
        return [v for v in self._store.values() if v.table_name == table_name]

    async def query(self, table_name: str, pk: str) -> list[DynamoItem]:
        return [v for v in self._store.values() if v.table_name == table_name and v.pk == pk]

    async def delete_all(self, table_name: str) -> None:
        self._store = {k: v for k, v in self._store.items() if v.table_name != table_name}
