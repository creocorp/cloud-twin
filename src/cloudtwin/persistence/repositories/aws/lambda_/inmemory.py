"""AWS Lambda — in-memory repository implementations."""

from __future__ import annotations

from typing import Optional

from cloudtwin.persistence.models.aws.lambda_ import LambdaFunction, LambdaInvocation
from cloudtwin.persistence.repositories.aws.lambda_.repository import (
    LambdaFunctionRepository,
    LambdaInvocationRepository,
)


class InMemoryLambdaFunctionRepository(LambdaFunctionRepository):
    def __init__(self):
        self._store: dict[str, LambdaFunction] = {}
        self._next_id = 1

    async def get(self, name: str) -> Optional[LambdaFunction]:
        return self._store.get(name)

    async def list_all(self) -> list[LambdaFunction]:
        return list(self._store.values())

    async def save(self, fn: LambdaFunction) -> LambdaFunction:
        if fn.name not in self._store:
            fn.id = self._next_id
            self._next_id += 1
        self._store[fn.name] = fn
        return fn

    async def delete(self, name: str) -> None:
        self._store.pop(name, None)


class InMemoryLambdaInvocationRepository(LambdaInvocationRepository):
    def __init__(self):
        self._store: list[LambdaInvocation] = []
        self._next_id = 1

    async def save(self, invocation: LambdaInvocation) -> LambdaInvocation:
        invocation.id = self._next_id
        self._next_id += 1
        self._store.append(invocation)
        return invocation
