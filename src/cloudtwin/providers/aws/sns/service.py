"""
SNS domain service.

Business logic for SNS operations. Has no knowledge of HTTP or XML.
Depends on repository interfaces only.
"""

from __future__ import annotations

import uuid
from datetime import datetime, timezone

from cloudtwin.core.errors import NotFoundError
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.persistence.models import SnsMessage, SnsSubscription, SnsTopic
from cloudtwin.persistence.repositories import (
    SnsMessageRepository,
    SnsSubscriptionRepository,
    SnsTopicRepository,
)

_ACCOUNT_ID = "000000000000"
_REGION = "us-east-1"


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


def _topic_arn(name: str) -> str:
    return f"arn:aws:sns:{_REGION}:{_ACCOUNT_ID}:{name}"


def _subscription_arn(topic_arn: str, suffix: str) -> str:
    return f"{topic_arn}:{suffix}"


class SnsService:
    def __init__(
        self,
        topic_repo: SnsTopicRepository,
        subscription_repo: SnsSubscriptionRepository,
        message_repo: SnsMessageRepository,
        telemetry: TelemetryEngine,
    ):
        self._topic_repo = topic_repo
        self._subscription_repo = subscription_repo
        self._message_repo = message_repo
        self._telemetry = telemetry

    # -------------------------------------------------------------------
    # Topics
    # -------------------------------------------------------------------

    async def create_topic(self, name: str) -> str:
        """Idempotent – returns the ARN of an existing topic if it already exists."""
        existing = await self._topic_repo.get_by_name(name)
        if existing:
            return existing.arn

        arn = _topic_arn(name)
        topic = SnsTopic(id=None, name=name, arn=arn, created_at=_now())
        await self._topic_repo.save(topic)
        await self._telemetry.emit("aws", "sns", "create_topic", {"name": name, "arn": arn})
        return arn

    async def list_topics(self) -> list[str]:
        """Return all topic ARNs."""
        topics = await self._topic_repo.list_all()
        return [t.arn for t in topics]

    async def delete_topic(self, topic_arn: str) -> None:
        """Delete a topic and its subscriptions."""
        topic = await self._topic_repo.get(topic_arn)
        if topic is None:
            raise NotFoundError(f"Topic not found: {topic_arn}")

        # delete subscriptions first
        subs = await self._subscription_repo.list_by_topic(topic_arn)
        for sub in subs:
            await self._subscription_repo.delete(sub.subscription_arn)

        await self._topic_repo.delete(topic_arn)
        await self._telemetry.emit("aws", "sns", "delete_topic", {"arn": topic_arn})

    # -------------------------------------------------------------------
    # Subscriptions
    # -------------------------------------------------------------------

    async def subscribe(self, topic_arn: str, protocol: str, endpoint: str) -> str:
        """Subscribe an endpoint to a topic. Returns the SubscriptionArn."""
        topic = await self._topic_repo.get(topic_arn)
        if topic is None:
            raise NotFoundError(f"Topic not found: {topic_arn}")

        # Check for duplicate (same topic + protocol + endpoint)
        existing = await self._subscription_repo.list_by_topic(topic_arn)
        for sub in existing:
            if sub.protocol == protocol and sub.endpoint == endpoint:
                return sub.subscription_arn

        sub_arn = _subscription_arn(topic_arn, str(uuid.uuid4()))
        subscription = SnsSubscription(
            id=None,
            subscription_arn=sub_arn,
            topic_arn=topic_arn,
            protocol=protocol,
            endpoint=endpoint,
            created_at=_now(),
        )
        await self._subscription_repo.save(subscription)
        await self._telemetry.emit(
            "aws", "sns", "subscribe",
            {"topic_arn": topic_arn, "protocol": protocol, "endpoint": endpoint},
        )
        return sub_arn

    async def unsubscribe(self, subscription_arn: str) -> None:
        """Unsubscribe an endpoint from a topic."""
        sub = await self._subscription_repo.get(subscription_arn)
        if sub is None:
            raise NotFoundError(f"Subscription not found: {subscription_arn}")

        await self._subscription_repo.delete(subscription_arn)
        await self._telemetry.emit("aws", "sns", "unsubscribe", {"subscription_arn": subscription_arn})

    async def list_subscriptions(self) -> list[str]:
        """Return all subscription ARNs."""
        subs = await self._subscription_repo.list_all()
        return [s.subscription_arn for s in subs]

    async def list_subscriptions_by_topic(self, topic_arn: str) -> list[str]:
        """Return all subscription ARNs for a specific topic."""
        topic = await self._topic_repo.get(topic_arn)
        if topic is None:
            raise NotFoundError(f"Topic not found: {topic_arn}")

        subs = await self._subscription_repo.list_by_topic(topic_arn)
        return [s.subscription_arn for s in subs]

    # -------------------------------------------------------------------
    # Publish
    # -------------------------------------------------------------------

    async def publish(
        self,
        topic_arn: str,
        message: str,
        subject: str | None = None,
    ) -> str:
        """Publish a message to a topic. Returns the MessageId."""
        topic = await self._topic_repo.get(topic_arn)
        if topic is None:
            raise NotFoundError(f"Topic not found: {topic_arn}")

        message_id = str(uuid.uuid4())
        msg = SnsMessage(
            id=None,
            message_id=message_id,
            topic_arn=topic_arn,
            message=message,
            subject=subject,
            created_at=_now(),
        )
        await self._message_repo.save(msg)
        await self._telemetry.emit(
            "aws", "sns", "publish",
            {"topic_arn": topic_arn, "message_id": message_id},
        )
        return message_id
