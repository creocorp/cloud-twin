"""AWS Lambda — abstract repository interfaces."""

from __future__ import annotations

from abc import ABC, abstractmethod
from typing import Optional

from cloudtwin.persistence.models.aws.lambda_ import LambdaFunction, LambdaInvocation


class LambdaFunctionRepository(ABC):
    @abstractmethod
    async def get(self, name: str) -> Optional[LambdaFunction]: ...
    @abstractmethod
    async def list_all(self) -> list[LambdaFunction]: ...
    @abstractmethod
    async def save(self, fn: LambdaFunction) -> LambdaFunction: ...
    @abstractmethod
    async def delete(self, name: str) -> None: ...


class LambdaInvocationRepository(ABC):
    @abstractmethod
    async def save(self, invocation: LambdaInvocation) -> LambdaInvocation: ...
