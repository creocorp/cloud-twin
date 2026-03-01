"""Azure Service Bus — in-memory repository implementations."""

from __future__ import annotations

from typing import Optional

from cloudtwin.persistence.models.azure.servicebus import (
    AsbMessage,
    AsbQueue,
    AsbSubscription,
    AsbTopic,
)
from cloudtwin.persistence.repositories.azure.servicebus.repository import (
    AsbMessageRepository,
    AsbQueueRepository,
    AsbSubscriptionRepository,
    AsbTopicRepository,
)


class InMemoryAsbQueueRepository(AsbQueueRepository):
    def __init__(self):
        self._store: dict[tuple[str, str], AsbQueue] = {}
        self._next_id = 1

    async def get(self, namespace: str, name: str) -> Optional[AsbQueue]:
        return self._store.get((namespace, name))

    async def list_by_namespace(self, namespace: str) -> list[AsbQueue]:
        return [q for (ns, _), q in self._store.items() if ns == namespace]

    async def save(self, queue: AsbQueue) -> AsbQueue:
        key = (queue.namespace, queue.name)
        if key not in self._store:
            queue.id = self._next_id
            self._next_id += 1
        self._store[key] = queue
        return queue

    async def delete(self, namespace: str, name: str) -> None:
        self._store.pop((namespace, name), None)


class InMemoryAsbTopicRepository(AsbTopicRepository):
    def __init__(self):
        self._store: dict[tuple[str, str], AsbTopic] = {}
        self._next_id = 1

    async def get(self, namespace: str, name: str) -> Optional[AsbTopic]:
        return self._store.get((namespace, name))

    async def list_by_namespace(self, namespace: str) -> list[AsbTopic]:
        return [t for (ns, _), t in self._store.items() if ns == namespace]

    async def save(self, topic: AsbTopic) -> AsbTopic:
        key = (topic.namespace, topic.name)
        if key not in self._store:
            topic.id = self._next_id
            self._next_id += 1
        self._store[key] = topic
        return topic

    async def delete(self, namespace: str, name: str) -> None:
        self._store.pop((namespace, name), None)


class InMemoryAsbSubscriptionRepository(AsbSubscriptionRepository):
    def __init__(self):
        self._store: dict[tuple[int, str], AsbSubscription] = {}
        self._next_id = 1

    async def get(self, topic_id: int, name: str) -> Optional[AsbSubscription]:
        return self._store.get((topic_id, name))

    async def list_by_topic(self, topic_id: int) -> list[AsbSubscription]:
        return [s for (tid, _), s in self._store.items() if tid == topic_id]

    async def save(self, sub: AsbSubscription) -> AsbSubscription:
        key = (sub.topic_id, sub.name)
        if key not in self._store:
            sub.id = self._next_id
            self._next_id += 1
        self._store[key] = sub
        return sub

    async def delete(self, topic_id: int, name: str) -> None:
        self._store.pop((topic_id, name), None)


class InMemoryAsbMessageRepository(AsbMessageRepository):
    def __init__(self):
        self._by_lock: dict[str, AsbMessage] = {}
        self._by_id: list[AsbMessage] = []
        self._next_id = 1

    async def save(self, message: AsbMessage) -> AsbMessage:
        message.id = self._next_id
        self._next_id += 1
        self._by_lock[message.lock_token] = message
        self._by_id.append(message)
        return message

    async def get_by_lock_token(self, lock_token: str) -> Optional[AsbMessage]:
        return self._by_lock.get(lock_token)

    async def get_active(
        self, entity_id: int, entity_type: str, limit: int = 1
    ) -> list[AsbMessage]:
        results = [
            m
            for m in self._by_id
            if m.entity_id == entity_id
            and m.entity_type == entity_type
            and m.state == "active"
        ]
        return results[:limit]

    async def update_state(self, lock_token: str, state: str) -> None:
        if lock_token in self._by_lock:
            self._by_lock[lock_token].state = state

    async def delete(self, lock_token: str) -> None:
        msg = self._by_lock.pop(lock_token, None)
        if msg and msg in self._by_id:
            self._by_id.remove(msg)
