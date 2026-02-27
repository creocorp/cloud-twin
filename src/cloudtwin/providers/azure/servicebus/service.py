"""Azure Service Bus – pure business logic."""

from __future__ import annotations

import logging
import uuid
from datetime import datetime, timezone
from typing import Optional

from cloudtwin.core.errors import NotFoundError
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.persistence.models.azure import AsbMessage, AsbQueue, AsbSubscription, AsbTopic
from cloudtwin.persistence.repositories.azure import (
    AsbMessageRepository,
    AsbQueueRepository,
    AsbSubscriptionRepository,
    AsbTopicRepository,
)

log = logging.getLogger("cloudtwin.azure.servicebus")


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class ServiceBusService:
    def __init__(
        self,
        namespace: str,
        queue_repo: AsbQueueRepository,
        topic_repo: AsbTopicRepository,
        subscription_repo: AsbSubscriptionRepository,
        message_repo: AsbMessageRepository,
        telemetry: TelemetryEngine,
    ):
        self._ns = namespace
        self._queues = queue_repo
        self._topics = topic_repo
        self._subscriptions = subscription_repo
        self._messages = message_repo
        self._telemetry = telemetry

    # ------------------------------------------------------------------
    # Queues
    # ------------------------------------------------------------------

    async def create_queue(self, name: str) -> AsbQueue:
        existing = await self._queues.get(self._ns, name)
        if existing:
            return existing
        queue = AsbQueue(namespace=self._ns, name=name, created_at=_now())
        result = await self._queues.save(queue)
        await self._telemetry.emit("azure", "servicebus", "create_queue", {"queue": name})
        return result

    async def get_queue(self, name: str) -> AsbQueue:
        queue = await self._queues.get(self._ns, name)
        if not queue:
            raise NotFoundError(f"Queue not found: {name}")
        return queue

    async def list_queues(self) -> list[AsbQueue]:
        return await self._queues.list_by_namespace(self._ns)

    async def delete_queue(self, name: str) -> None:
        await self.get_queue(name)
        await self._queues.delete(self._ns, name)
        await self._telemetry.emit("azure", "servicebus", "delete_queue", {"queue": name})

    # ------------------------------------------------------------------
    # Topics
    # ------------------------------------------------------------------

    async def create_topic(self, name: str) -> AsbTopic:
        existing = await self._topics.get(self._ns, name)
        if existing:
            return existing
        topic = AsbTopic(namespace=self._ns, name=name, created_at=_now())
        result = await self._topics.save(topic)
        await self._telemetry.emit("azure", "servicebus", "create_topic", {"topic": name})
        return result

    async def get_topic(self, name: str) -> AsbTopic:
        topic = await self._topics.get(self._ns, name)
        if not topic:
            raise NotFoundError(f"Topic not found: {name}")
        return topic

    async def list_topics(self) -> list[AsbTopic]:
        return await self._topics.list_by_namespace(self._ns)

    async def delete_topic(self, name: str) -> None:
        await self.get_topic(name)
        await self._topics.delete(self._ns, name)
        await self._telemetry.emit("azure", "servicebus", "delete_topic", {"topic": name})

    # ------------------------------------------------------------------
    # Subscriptions
    # ------------------------------------------------------------------

    async def create_subscription(self, topic_name: str, sub_name: str) -> AsbSubscription:
        topic = await self.get_topic(topic_name)
        existing = await self._subscriptions.get(topic.id, sub_name)
        if existing:
            return existing
        sub = AsbSubscription(topic_id=topic.id, name=sub_name, created_at=_now())
        result = await self._subscriptions.save(sub)
        await self._telemetry.emit("azure", "servicebus", "create_subscription", {
            "topic": topic_name, "subscription": sub_name
        })
        return result

    async def get_subscription(self, topic_name: str, sub_name: str) -> AsbSubscription:
        topic = await self.get_topic(topic_name)
        sub = await self._subscriptions.get(topic.id, sub_name)
        if not sub:
            raise NotFoundError(f"Subscription not found: {topic_name}/{sub_name}")
        return sub

    async def list_subscriptions(self, topic_name: str) -> list[AsbSubscription]:
        topic = await self.get_topic(topic_name)
        return await self._subscriptions.list_by_topic(topic.id)

    async def delete_subscription(self, topic_name: str, sub_name: str) -> None:
        sub = await self.get_subscription(topic_name, sub_name)
        await self._subscriptions.delete(sub.topic_id, sub_name)

    # ------------------------------------------------------------------
    # Messages – queues
    # ------------------------------------------------------------------

    async def send_to_queue(self, queue_name: str, body: str, content_type: str = "text/plain") -> AsbMessage:
        queue = await self.get_queue(queue_name)
        msg = AsbMessage(
            message_id=str(uuid.uuid4()),
            entity_id=queue.id,
            entity_type="queue",
            body=body,
            content_type=content_type,
            lock_token=str(uuid.uuid4()),
            state="active",
            delivery_count=0,
            created_at=_now(),
        )
        result = await self._messages.save(msg)
        await self._telemetry.emit("azure", "servicebus", "send_queue_message", {"queue": queue_name})
        return result

    async def receive_from_queue(self, queue_name: str, limit: int = 1) -> list[AsbMessage]:
        queue = await self.get_queue(queue_name)
        messages = await self._messages.get_active(queue.id, "queue", limit=limit)
        for msg in messages:
            await self._messages.update_state(msg.lock_token, "locked")
            msg.state = "locked"
        return messages

    # ------------------------------------------------------------------
    # Messages – topics / subscriptions
    # ------------------------------------------------------------------

    async def send_to_topic(self, topic_name: str, body: str, content_type: str = "text/plain") -> list[AsbMessage]:
        topic = await self.get_topic(topic_name)
        subs = await self._subscriptions.list_by_topic(topic.id)
        results = []
        for sub in subs:
            msg = AsbMessage(
                message_id=str(uuid.uuid4()),
                entity_id=sub.id,
                entity_type="subscription",
                body=body,
                content_type=content_type,
                lock_token=str(uuid.uuid4()),
                state="active",
                delivery_count=0,
                created_at=_now(),
            )
            results.append(await self._messages.save(msg))
        await self._telemetry.emit("azure", "servicebus", "send_topic_message", {
            "topic": topic_name, "fan_out": len(results)
        })
        return results

    async def receive_from_subscription(
        self, topic_name: str, sub_name: str, limit: int = 1
    ) -> list[AsbMessage]:
        sub = await self.get_subscription(topic_name, sub_name)
        messages = await self._messages.get_active(sub.id, "subscription", limit=limit)
        for msg in messages:
            await self._messages.update_state(msg.lock_token, "locked")
            msg.state = "locked"
        return messages

    # ------------------------------------------------------------------
    # Message settlement
    # ------------------------------------------------------------------

    async def complete_message(self, lock_token: str) -> None:
        msg = await self._messages.get_by_lock_token(lock_token)
        if not msg:
            raise NotFoundError(f"Message not found: {lock_token}")
        await self._messages.delete(lock_token)

    async def abandon_message(self, lock_token: str) -> None:
        msg = await self._messages.get_by_lock_token(lock_token)
        if not msg:
            raise NotFoundError(f"Message not found: {lock_token}")
        await self._messages.update_state(lock_token, "active")

    async def deadletter_message(self, lock_token: str) -> None:
        msg = await self._messages.get_by_lock_token(lock_token)
        if not msg:
            raise NotFoundError(f"Message not found: {lock_token}")
        await self._messages.update_state(lock_token, "deadletter")
