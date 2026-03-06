"""AWS DynamoDB domain model dataclasses."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional


@dataclass
class DynamoTable:
    name: str
    key_schema: str  # JSON-serialised list of KeySchemaElement
    attribute_definitions: str  # JSON-serialised list of AttributeDefinition
    created_at: str
    id: Optional[int] = None


@dataclass
class DynamoItem:
    table_name: str
    pk: str  # partition key value (stringified)
    sk: str  # sort key value (or "" if no sort key)
    item: str  # full item JSON blob
    created_at: str
    id: Optional[int] = None
