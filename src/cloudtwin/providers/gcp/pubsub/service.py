"""GCP Pub/Sub – pure business logic."""

from __future__ import annotations

import base64
import logging
import uuid
from datetime import datetime, timezone
from typing import Optional

from cloudtwin.core.errors import NotFoundError, ValidationError
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.persistence.models.gcp import (
    PubsubAckable,
    PubsubMessage,
    PubsubSubscription,
    PubsubTopic,
)
from cloudtwin.persistence.repositories.gcp import (
    PubsubAckableRepository,
    PubsubMessageRepository,
    PubsubSubscriptionRepository,
    PubsubTopicRepository,
)

log = logging.getLogger("cloudtwin.gcp.pubsub")


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class PubsubService:
    def __init__(
        self,
        project: str,
        topic_repo: PubsubTopicRepository,
        subscription_repo: PubsubSubscriptionRepository,
        message_repo: PubsubMessageRepository,
        ackable_repo: PubsubAckableRepository,
        telemetry: TelemetryEngine,
    ):
        self._project = project
        self._topics = topic_repo
        self._subscriptions = subscription_repo
        self._messages = message_repo
        self._ackables = ackable_repo
        self._telemetry = telemetry

    def _topic_full(self, name: str) -> str:
        if name.startswith("projects/"):
            return name
        return f"projects/{self._project}/topics/{name}"

    def _sub_full(self, name: str) -> str:
        if name.startswith("projects/"):
            return name
        return f"projects/{self._project}/subscriptions/{name}"

    # ------------------------------------------------------------------
    # Topics
    # ------------------------------------------------------------------

    async def create_topic(self, full_name: str) -> PubsubTopic:
        existing = await self._topics.get(full_name)
        if existing:
            return existing
        parts = full_name.split("/")
        name = parts[-1]
        project = parts[1] if len(parts) >= 2 else self._project
        topic = PubsubTopic(
            project=project, name=name, full_name=full_name, created_at=_now()
        )
        result = await self._topics.save(topic)
        await self._telemetry.emit("gcp", "pubsub", "create_topic", {"topic": full_name})
        return result

    async def get_topic(self, full_name: str) -> PubsubTopic:
        topic = await self._topics.get(full_name)
        if not topic:
            raise NotFoundError(f"Topic not found: {full_name}")
        return topic

    async def list_topics(self, project: Optional[str] = None) -> list[PubsubTopic]:
        return await self._topics.list_by_project(project or self._project)

    async def delete_topic(self, full_name: str) -> None:
        await self.get_topic(full_name)
        await self._topics.delete(full_name)
        await self._telemetry.emit("gcp", "pubsub", "delete_topic", {"topic": full_name})

    # ------------------------------------------------------------------
    # Subscriptions
    # ------------------------------------------------------------------

    async def create_subscription(
        self,
        full_name: str,
        topic_full_name: str,
        ack_deadline_seconds: int = 10,
    ) -> PubsubSubscription:
        existing = await self._subscriptions.get(full_name)
        if existing:
            return existing
        # Verify topic exists
        await self.get_topic(topic_full_name)
        parts = full_name.split("/")
        name = parts[-1]
        project = parts[1] if len(parts) >= 2 else self._project
        sub = PubsubSubscription(
            project=project,
            name=name,
            full_name=full_name,
            topic_full_name=topic_full_name,
            ack_deadline_seconds=ack_deadline_seconds,
            created_at=_now(),
        )
        result = await self._subscriptions.save(sub)
        await self._telemetry.emit("gcp", "pubsub", "create_subscription", {
            "subscription": full_name, "topic": topic_full_name
        })
        return result

    async def get_subscription(self, full_name: str) -> PubsubSubscription:
        sub = await self._subscriptions.get(full_name)
        if not sub:
            raise NotFoundError(f"Subscription not found: {full_name}")
        return sub

    async def list_subscriptions(self, project: Optional[str] = None) -> list[PubsubSubscription]:
        return await self._subscriptions.list_by_project(project or self._project)

    async def delete_subscription(self, full_name: str) -> None:
        await self.get_subscription(full_name)
        await self._subscriptions.delete(full_name)
        await self._telemetry.emit("gcp", "pubsub", "delete_subscription", {"subscription": full_name})

    # ------------------------------------------------------------------
    # Publish
    # ------------------------------------------------------------------

    async def publish(self, topic_full_name: str, messages: list[dict]) -> list[str]:
        """Publish messages, fan out to all subscriptions.
        Returns list of published message IDs.
        """
        await self.get_topic(topic_full_name)
        subs = await self._subscriptions.list_by_topic(topic_full_name)
        message_ids = []
        for msg_data in messages:
            msg_id = str(uuid.uuid4())
            import json
            msg = PubsubMessage(
                message_id=msg_id,
                topic_full_name=topic_full_name,
                data=msg_data.get("data", ""),  # already base64
                attributes=json.dumps(msg_data.get("attributes", {})),
                created_at=_now(),
            )
            await self._messages.save(msg)
            # Fan out to subscriptions
            for sub in subs:
                ackable = PubsubAckable(
                    ack_id=str(uuid.uuid4()),
                    message_id=msg_id,
                    subscription_full_name=sub.full_name,
                    delivery_attempt=1,
                    ack_deadline_seconds=sub.ack_deadline_seconds,
                    created_at=_now(),
                )
                await self._ackables.save(ackable)
            message_ids.append(msg_id)
        await self._telemetry.emit("gcp", "pubsub", "publish", {
            "topic": topic_full_name, "count": len(messages)
        })
        return message_ids

    # ------------------------------------------------------------------
    # Pull
    # ------------------------------------------------------------------

    async def pull(self, subscription_full_name: str, max_messages: int = 10) -> list[dict]:
        """Pull pending messages. Returns list of receivedMessages dicts."""
        import json
        await self.get_subscription(subscription_full_name)
        ackables = await self._ackables.get_pending(subscription_full_name, limit=max_messages)
        result = []
        for ackable in ackables:
            msg = await self._messages.get(ackable.message_id)
            if msg is None:
                continue
            # attributes stored as JSON string, must be a dict in response
            attributes = {}
            if msg.attributes:
                try:
                    attributes = json.loads(msg.attributes)
                except (ValueError, TypeError):
                    attributes = {}
            result.append({
                "ackId": ackable.ack_id,
                "message": {
                    "messageId": msg.message_id,
                    "data": msg.data or "",
                    "attributes": attributes,
                    "publishTime": msg.created_at,
                },
                "deliveryAttempt": ackable.delivery_attempt,
            })
        return result

    # ------------------------------------------------------------------
    # Acknowledge
    # ------------------------------------------------------------------

    async def acknowledge(self, subscription_full_name: str, ack_ids: list[str]) -> None:
        for ack_id in ack_ids:
            await self._ackables.delete(ack_id)
        await self._telemetry.emit("gcp", "pubsub", "acknowledge", {
            "subscription": subscription_full_name, "count": len(ack_ids)
        })
