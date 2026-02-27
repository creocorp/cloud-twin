"""GCP Cloud Functions — abstract repository interfaces."""

from __future__ import annotations

from abc import ABC, abstractmethod
from typing import Optional

from cloudtwin.persistence.models.gcp.cloudfunctions import CloudFunction, CloudFunctionInvocation


class CloudFunctionRepository(ABC):
    @abstractmethod
    async def get(self, full_name: str) -> Optional[CloudFunction]: ...
    @abstractmethod
    async def list_by_project(self, project: str) -> list[CloudFunction]: ...
    @abstractmethod
    async def save(self, fn: CloudFunction) -> CloudFunction: ...
    @abstractmethod
    async def delete(self, full_name: str) -> None: ...


class CloudFunctionInvocationRepository(ABC):
    @abstractmethod
    async def save(self, inv: CloudFunctionInvocation) -> CloudFunctionInvocation: ...
