"""SES — in-memory repository implementations."""

from __future__ import annotations

from typing import Optional

from cloudtwin.persistence.models.aws.ses import SesIdentity, SesMessage
from cloudtwin.persistence.repositories.aws.ses.repository import (
    SesIdentityRepository,
    SesMessageRepository,
)


class InMemorySesIdentityRepository(SesIdentityRepository):
    def __init__(self):
        self._store: dict[str, SesIdentity] = {}

    async def get(self, identity: str) -> Optional[SesIdentity]:
        return self._store.get(identity)

    async def list_all(self) -> list[SesIdentity]:
        return list(self._store.values())

    async def save(self, identity: SesIdentity) -> SesIdentity:
        self._store[identity.identity] = identity
        return identity

    async def delete(self, identity: str) -> None:
        self._store.pop(identity, None)


class InMemorySesMessageRepository(SesMessageRepository):
    def __init__(self):
        self._store: dict[str, SesMessage] = {}

    async def save(self, message: SesMessage) -> SesMessage:
        self._store[message.message_id] = message
        return message

    async def list_all(self) -> list[SesMessage]:
        return list(self._store.values())

    async def get(self, message_id: str) -> Optional[SesMessage]:
        return self._store.get(message_id)
