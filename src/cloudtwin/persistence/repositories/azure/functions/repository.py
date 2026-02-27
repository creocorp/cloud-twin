"""Azure Functions — abstract repository interfaces."""

from __future__ import annotations

from abc import ABC, abstractmethod
from typing import Optional

from cloudtwin.persistence.models.azure.functions import AzureFunction, AzureFunctionInvocation


class AzureFunctionRepository(ABC):
    @abstractmethod
    async def get(self, app: str, name: str) -> Optional[AzureFunction]: ...
    @abstractmethod
    async def list_by_app(self, app: str) -> list[AzureFunction]: ...
    @abstractmethod
    async def save(self, fn: AzureFunction) -> AzureFunction: ...
    @abstractmethod
    async def delete(self, app: str, name: str) -> None: ...


class AzureFunctionInvocationRepository(ABC):
    @abstractmethod
    async def save(self, inv: AzureFunctionInvocation) -> AzureFunctionInvocation: ...
