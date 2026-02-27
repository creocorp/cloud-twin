"""SNS — in-memory repository implementations."""

from __future__ import annotations

from typing import Optional

from cloudtwin.persistence.models.aws.sns import SnsMessage, SnsSubscription, SnsTopic
from cloudtwin.persistence.repositories.aws.sns.repository import (
    SnsMessageRepository,
    SnsSubscriptionRepository,
    SnsTopicRepository,
)


class InMemorySnsTopicRepository(SnsTopicRepository):
    def __init__(self):
        self._by_arn: dict[str, SnsTopic] = {}
        self._next_id = 1

    async def get(self, arn: str) -> Optional[SnsTopic]:
        return self._by_arn.get(arn)

    async def get_by_name(self, name: str) -> Optional[SnsTopic]:
        return next((t for t in self._by_arn.values() if t.name == name), None)

    async def list_all(self) -> list[SnsTopic]:
        return list(self._by_arn.values())

    async def save(self, topic: SnsTopic) -> SnsTopic:
        if topic.arn not in self._by_arn:
            topic.id = self._next_id
            self._next_id += 1
        self._by_arn[topic.arn] = topic
        return topic


class InMemorySnsSubscriptionRepository(SnsSubscriptionRepository):
    def __init__(self):
        self._store: dict[str, SnsSubscription] = {}
        self._next_id = 1

    async def get(self, subscription_arn: str) -> Optional[SnsSubscription]:
        return self._store.get(subscription_arn)

    async def list_by_topic(self, topic_arn: str) -> list[SnsSubscription]:
        return [s for s in self._store.values() if s.topic_arn == topic_arn]

    async def save(self, sub: SnsSubscription) -> SnsSubscription:
        if sub.subscription_arn not in self._store:
            sub.id = self._next_id
            self._next_id += 1
        self._store[sub.subscription_arn] = sub
        return sub


class InMemorySnsMessageRepository(SnsMessageRepository):
    def __init__(self):
        self._store: list[SnsMessage] = []
        self._next_id = 1

    async def save(self, message: SnsMessage) -> SnsMessage:
        message.id = self._next_id
        self._next_id += 1
        self._store.append(message)
        return message

    async def list_by_topic(self, topic_arn: str) -> list[SnsMessage]:
        return [m for m in self._store if m.topic_arn == topic_arn]
