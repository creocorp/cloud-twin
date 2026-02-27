"""AWS DynamoDB — abstract repository interfaces."""

from __future__ import annotations

from abc import ABC, abstractmethod
from typing import Optional

from cloudtwin.persistence.models.aws.dynamodb import DynamoItem, DynamoTable


class DynamoTableRepository(ABC):
    @abstractmethod
    async def get(self, name: str) -> Optional[DynamoTable]: ...
    @abstractmethod
    async def list_all(self) -> list[DynamoTable]: ...
    @abstractmethod
    async def save(self, table: DynamoTable) -> DynamoTable: ...
    @abstractmethod
    async def delete(self, name: str) -> None: ...


class DynamoItemRepository(ABC):
    @abstractmethod
    async def put(self, item: DynamoItem) -> DynamoItem: ...
    @abstractmethod
    async def get(self, table_name: str, pk: str, sk: str) -> Optional[DynamoItem]: ...
    @abstractmethod
    async def delete(self, table_name: str, pk: str, sk: str) -> None: ...
    @abstractmethod
    async def scan(self, table_name: str) -> list[DynamoItem]: ...
    @abstractmethod
    async def query(self, table_name: str, pk: str) -> list[DynamoItem]: ...
    @abstractmethod
    async def delete_all(self, table_name: str) -> None: ...
