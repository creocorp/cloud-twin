"""Azure Event Grid — pure business logic."""

from __future__ import annotations

import json
import logging
import uuid
from datetime import datetime, timezone

from cloudtwin.core.errors import NotFoundError
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.persistence.models.azure.eventgrid import EventGridEvent, EventGridTopic
from cloudtwin.persistence.repositories.azure.eventgrid import (
    EventGridEventRepository,
    EventGridTopicRepository,
)

log = logging.getLogger("cloudtwin.azure.eventgrid")


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class EventGridService:
    def __init__(
        self,
        topic_repo: EventGridTopicRepository,
        event_repo: EventGridEventRepository,
        telemetry: TelemetryEngine,
    ):
        self._topics = topic_repo
        self._events = event_repo
        self._telemetry = telemetry

    async def create_topic(self, name: str, endpoint: str = "") -> EventGridTopic:
        existing = await self._topics.get(name)
        if existing:
            return existing
        topic = EventGridTopic(name=name, endpoint=endpoint, created_at=_now())
        saved = await self._topics.save(topic)
        await self._telemetry.emit("azure", "eventgrid", "create_topic", {"name": name})
        return saved

    async def delete_topic(self, name: str) -> None:
        topic = await self._topics.get(name)
        if not topic:
            raise NotFoundError(f"Topic not found: {name}")
        await self._topics.delete(name)
        await self._telemetry.emit("azure", "eventgrid", "delete_topic", {"name": name})

    async def list_topics(self) -> list[EventGridTopic]:
        return await self._topics.list_all()

    async def publish_events(self, topic_name: str, events: list[dict]) -> int:
        topic = await self._topics.get(topic_name)
        if not topic:
            raise NotFoundError(f"Topic not found: {topic_name}")
        count = 0
        for raw in events:
            event = EventGridEvent(
                topic_name=topic_name,
                event_id=raw.get("id", str(uuid.uuid4())),
                event_type=raw.get("eventType", ""),
                subject=raw.get("subject", ""),
                data=json.dumps(raw.get("data", {})),
                created_at=_now(),
            )
            await self._events.save(event)
            count += 1
        await self._telemetry.emit(
            "azure",
            "eventgrid",
            "publish_events",
            {"topic": topic_name, "count": count},
        )
        return count

    async def list_events(self, topic_name: str) -> list[EventGridEvent]:
        return await self._events.list_by_topic(topic_name)
