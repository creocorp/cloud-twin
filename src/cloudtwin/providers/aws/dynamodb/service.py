"""AWS DynamoDB — pure business logic."""

from __future__ import annotations

import json
import logging
import uuid
from datetime import datetime, timezone

from cloudtwin.core.errors import NotFoundError
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.persistence.models.aws.dynamodb import DynamoItem, DynamoTable
from cloudtwin.persistence.repositories.aws.dynamodb import (
    DynamoItemRepository,
    DynamoTableRepository,
)

log = logging.getLogger("cloudtwin.aws.dynamodb")

_ACCOUNT_ID = "000000000000"
_REGION = "us-east-1"


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


def _arn(name: str) -> str:
    return f"arn:aws:dynamodb:{_REGION}:{_ACCOUNT_ID}:table/{name}"


class DynamoDBService:
    def __init__(
        self,
        table_repo: DynamoTableRepository,
        item_repo: DynamoItemRepository,
        telemetry: TelemetryEngine,
    ):
        self._tables = table_repo
        self._items = item_repo
        self._telemetry = telemetry

    # ------------------------------------------------------------------ tables

    async def create_table(
        self, name: str, key_schema: list, attribute_definitions: list
    ) -> DynamoTable:
        existing = await self._tables.get(name)
        if existing:
            return existing
        table = DynamoTable(
            name=name,
            key_schema=json.dumps(key_schema),
            attribute_definitions=json.dumps(attribute_definitions),
            created_at=_now(),
        )
        saved = await self._tables.save(table)
        await self._telemetry.emit("aws", "dynamodb", "create_table", {"name": name})
        return saved

    async def describe_table(self, name: str) -> DynamoTable:
        table = await self._tables.get(name)
        if not table:
            raise NotFoundError(f"Table not found: {name}")
        return table

    async def list_tables(self) -> list[str]:
        return [t.name for t in await self._tables.list_all()]

    async def delete_table(self, name: str) -> None:
        table = await self._tables.get(name)
        if not table:
            raise NotFoundError(f"Table not found: {name}")
        await self._items.delete_all(name)
        await self._tables.delete(name)
        await self._telemetry.emit("aws", "dynamodb", "delete_table", {"name": name})

    # ------------------------------------------------------------------ items

    def _extract_key(self, table: DynamoTable, item: dict) -> tuple[str, str]:
        schema = json.loads(table.key_schema)
        pk_name = next(k["AttributeName"] for k in schema if k["KeyType"] == "HASH")
        sk_entry = next((k for k in schema if k["KeyType"] == "RANGE"), None)
        sk_name = sk_entry["AttributeName"] if sk_entry else None
        pk_val = json.dumps(item.get(pk_name, {}))
        sk_val = json.dumps(item.get(sk_name, {})) if sk_name else ""
        return pk_val, sk_val

    async def put_item(self, table_name: str, item: dict) -> None:
        table = await self._tables.get(table_name)
        if not table:
            raise NotFoundError(f"Table not found: {table_name}")
        pk, sk = self._extract_key(table, item)
        dynamo_item = DynamoItem(
            table_name=table_name,
            pk=pk,
            sk=sk,
            item=json.dumps(item),
            created_at=_now(),
        )
        await self._items.put(dynamo_item)
        await self._telemetry.emit("aws", "dynamodb", "put_item", {"table": table_name})

    async def get_item(self, table_name: str, key: dict) -> dict | None:
        table = await self._tables.get(table_name)
        if not table:
            raise NotFoundError(f"Table not found: {table_name}")
        pk, sk = self._extract_key(table, key)
        item = await self._items.get(table_name, pk, sk)
        return json.loads(item.item) if item else None

    async def delete_item(self, table_name: str, key: dict) -> None:
        table = await self._tables.get(table_name)
        if not table:
            raise NotFoundError(f"Table not found: {table_name}")
        pk, sk = self._extract_key(table, key)
        await self._items.delete(table_name, pk, sk)
        await self._telemetry.emit(
            "aws", "dynamodb", "delete_item", {"table": table_name}
        )

    async def scan(self, table_name: str) -> list[dict]:
        table = await self._tables.get(table_name)
        if not table:
            raise NotFoundError(f"Table not found: {table_name}")
        items = await self._items.scan(table_name)
        return [json.loads(i.item) for i in items]

    async def query(self, table_name: str, key_condition: dict) -> list[dict]:
        """Simple PK-only equality query."""
        table = await self._tables.get(table_name)
        if not table:
            raise NotFoundError(f"Table not found: {table_name}")
        schema = json.loads(table.key_schema)
        pk_name = next(k["AttributeName"] for k in schema if k["KeyType"] == "HASH")
        pk_val = json.dumps(key_condition.get(pk_name, {}))
        items = await self._items.query(table_name, pk_val)
        return [json.loads(i.item) for i in items]
