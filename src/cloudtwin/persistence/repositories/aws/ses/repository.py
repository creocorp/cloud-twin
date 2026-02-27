"""SES — abstract repository interfaces."""

from __future__ import annotations

from abc import ABC, abstractmethod
from typing import Optional

from cloudtwin.persistence.models.aws.ses import SesIdentity, SesMessage


class SesIdentityRepository(ABC):
    @abstractmethod
    async def get(self, identity: str) -> Optional[SesIdentity]: ...
    @abstractmethod
    async def list_all(self) -> list[SesIdentity]: ...
    @abstractmethod
    async def save(self, identity: SesIdentity) -> SesIdentity: ...
    @abstractmethod
    async def delete(self, identity: str) -> None: ...


class SesMessageRepository(ABC):
    @abstractmethod
    async def save(self, message: SesMessage) -> SesMessage: ...
    @abstractmethod
    async def list_all(self) -> list[SesMessage]: ...
    @abstractmethod
    async def get(self, message_id: str) -> Optional[SesMessage]: ...
