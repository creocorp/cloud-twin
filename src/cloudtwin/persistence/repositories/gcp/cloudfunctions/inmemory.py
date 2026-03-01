"""GCP Cloud Functions — in-memory repository implementations."""

from __future__ import annotations

from typing import Optional

from cloudtwin.persistence.models.gcp.cloudfunctions import (
    CloudFunction,
    CloudFunctionInvocation,
)
from cloudtwin.persistence.repositories.gcp.cloudfunctions.repository import (
    CloudFunctionInvocationRepository,
    CloudFunctionRepository,
)


class InMemoryCloudFunctionRepository(CloudFunctionRepository):
    def __init__(self):
        self._store: dict[str, CloudFunction] = {}
        self._next_id = 1

    async def get(self, full_name: str) -> Optional[CloudFunction]:
        return self._store.get(full_name)

    async def list_by_project(self, project: str) -> list[CloudFunction]:
        return [f for f in self._store.values() if f.project == project]

    async def save(self, fn: CloudFunction) -> CloudFunction:
        if fn.full_name not in self._store:
            fn.id = self._next_id
            self._next_id += 1
        self._store[fn.full_name] = fn
        return fn

    async def delete(self, full_name: str) -> None:
        self._store.pop(full_name, None)


class InMemoryCloudFunctionInvocationRepository(CloudFunctionInvocationRepository):
    def __init__(self):
        self._store: list[CloudFunctionInvocation] = []
        self._next_id = 1

    async def save(self, inv: CloudFunctionInvocation) -> CloudFunctionInvocation:
        inv.id = self._next_id
        self._next_id += 1
        self._store.append(inv)
        return inv
