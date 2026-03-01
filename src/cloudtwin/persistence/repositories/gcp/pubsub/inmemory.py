"""GCP Pub/Sub — in-memory repository implementations."""

from __future__ import annotations

from typing import Optional

from cloudtwin.persistence.models.gcp.pubsub import (
    PubsubAckable,
    PubsubMessage,
    PubsubSubscription,
    PubsubTopic,
)
from cloudtwin.persistence.repositories.gcp.pubsub.repository import (
    PubsubAckableRepository,
    PubsubMessageRepository,
    PubsubSubscriptionRepository,
    PubsubTopicRepository,
)


class InMemoryPubsubTopicRepository(PubsubTopicRepository):
    def __init__(self):
        self._store: dict[str, PubsubTopic] = {}
        self._next_id = 1

    async def get(self, full_name: str) -> Optional[PubsubTopic]:
        return self._store.get(full_name)

    async def list_by_project(self, project: str) -> list[PubsubTopic]:
        return [t for t in self._store.values() if t.project == project]

    async def save(self, topic: PubsubTopic) -> PubsubTopic:
        if topic.full_name not in self._store:
            topic.id = self._next_id
            self._next_id += 1
        self._store[topic.full_name] = topic
        return topic

    async def delete(self, full_name: str) -> None:
        self._store.pop(full_name, None)


class InMemoryPubsubSubscriptionRepository(PubsubSubscriptionRepository):
    def __init__(self):
        self._store: dict[str, PubsubSubscription] = {}
        self._next_id = 1

    async def get(self, full_name: str) -> Optional[PubsubSubscription]:
        return self._store.get(full_name)

    async def list_by_topic(self, topic_full_name: str) -> list[PubsubSubscription]:
        return [s for s in self._store.values() if s.topic_full_name == topic_full_name]

    async def list_by_project(self, project: str) -> list[PubsubSubscription]:
        return [s for s in self._store.values() if s.project == project]

    async def save(self, sub: PubsubSubscription) -> PubsubSubscription:
        if sub.full_name not in self._store:
            sub.id = self._next_id
            self._next_id += 1
        self._store[sub.full_name] = sub
        return sub

    async def delete(self, full_name: str) -> None:
        self._store.pop(full_name, None)


class InMemoryPubsubMessageRepository(PubsubMessageRepository):
    def __init__(self):
        self._store: dict[str, PubsubMessage] = {}
        self._next_id = 1

    async def save(self, message: PubsubMessage) -> PubsubMessage:
        message.id = self._next_id
        self._next_id += 1
        self._store[message.message_id] = message
        return message

    async def list_by_topic(self, topic_full_name: str) -> list[PubsubMessage]:
        return [m for m in self._store.values() if m.topic_full_name == topic_full_name]

    async def get(self, message_id: str) -> Optional[PubsubMessage]:
        return self._store.get(message_id)


class InMemoryPubsubAckableRepository(PubsubAckableRepository):
    def __init__(self):
        self._store: dict[str, PubsubAckable] = {}
        self._ordered: list[PubsubAckable] = []
        self._next_id = 1

    async def save(self, ackable: PubsubAckable) -> PubsubAckable:
        ackable.id = self._next_id
        self._next_id += 1
        self._store[ackable.ack_id] = ackable
        self._ordered.append(ackable)
        return ackable

    async def get_by_ack_id(self, ack_id: str) -> Optional[PubsubAckable]:
        return self._store.get(ack_id)

    async def get_pending(
        self, subscription_full_name: str, limit: int = 10
    ) -> list[PubsubAckable]:
        results = [
            a
            for a in self._ordered
            if a.subscription_full_name == subscription_full_name
        ]
        return results[:limit]

    async def delete(self, ack_id: str) -> None:
        ackable = self._store.pop(ack_id, None)
        if ackable and ackable in self._ordered:
            self._ordered.remove(ackable)
