"""Azure Functions — in-memory repository implementations."""

from __future__ import annotations

from typing import Optional

from cloudtwin.persistence.models.azure.functions import (
    AzureFunction,
    AzureFunctionInvocation,
)
from cloudtwin.persistence.repositories.azure.functions.repository import (
    AzureFunctionInvocationRepository,
    AzureFunctionRepository,
)


class InMemoryAzureFunctionRepository(AzureFunctionRepository):
    def __init__(self):
        self._store: dict[tuple[str, str], AzureFunction] = {}
        self._next_id = 1

    async def get(self, app: str, name: str) -> Optional[AzureFunction]:
        return self._store.get((app, name))

    async def list_by_app(self, app: str) -> list[AzureFunction]:
        return [f for f in self._store.values() if f.app == app]

    async def save(self, fn: AzureFunction) -> AzureFunction:
        k = (fn.app, fn.name)
        if k not in self._store:
            fn.id = self._next_id
            self._next_id += 1
        self._store[k] = fn
        return fn

    async def delete(self, app: str, name: str) -> None:
        self._store.pop((app, name), None)


class InMemoryAzureFunctionInvocationRepository(AzureFunctionInvocationRepository):
    def __init__(self):
        self._store: list[AzureFunctionInvocation] = []
        self._next_id = 1

    async def save(self, inv: AzureFunctionInvocation) -> AzureFunctionInvocation:
        inv.id = self._next_id
        self._next_id += 1
        self._store.append(inv)
        return inv
