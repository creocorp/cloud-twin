"""GCP Cloud Tasks — pure business logic."""

from __future__ import annotations

import json
import logging
import uuid
from datetime import datetime, timezone

from cloudtwin.core.errors import NotFoundError
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.persistence.models.gcp.cloudtasks import CloudTask, CloudTasksQueue
from cloudtwin.persistence.repositories.gcp.cloudtasks import (
    CloudTaskRepository,
    CloudTasksQueueRepository,
)

log = logging.getLogger("cloudtwin.gcp.cloudtasks")


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


def _full_name(project: str, location: str, name: str) -> str:
    return f"projects/{project}/locations/{location}/queues/{name}"


class CloudTasksService:
    def __init__(
        self,
        queue_repo: CloudTasksQueueRepository,
        task_repo: CloudTaskRepository,
        telemetry: TelemetryEngine,
    ):
        self._queues = queue_repo
        self._tasks = task_repo
        self._telemetry = telemetry

    async def create_queue(
        self, project: str, location: str, name: str
    ) -> CloudTasksQueue:
        full_name = _full_name(project, location, name)
        existing = await self._queues.get(full_name)
        if existing:
            return existing
        queue = CloudTasksQueue(
            project=project,
            location=location,
            name=name,
            full_name=full_name,
            created_at=_now(),
        )
        saved = await self._queues.save(queue)
        await self._telemetry.emit(
            "gcp", "cloudtasks", "create_queue", {"project": project, "name": name}
        )
        return saved

    async def get_queue(
        self, project: str, location: str, name: str
    ) -> CloudTasksQueue:
        queue = await self._queues.get(_full_name(project, location, name))
        if not queue:
            raise NotFoundError(f"Queue not found: {name}")
        return queue

    async def list_queues(self, project: str) -> list[CloudTasksQueue]:
        return await self._queues.list_by_project(project)

    async def delete_queue(self, project: str, location: str, name: str) -> None:
        queue = await self._queues.get(_full_name(project, location, name))
        if not queue:
            raise NotFoundError(f"Queue not found: {name}")
        await self._queues.delete(_full_name(project, location, name))
        await self._telemetry.emit(
            "gcp", "cloudtasks", "delete_queue", {"project": project, "name": name}
        )

    async def create_task(
        self, project: str, location: str, queue_name: str, payload: dict
    ) -> CloudTask:
        queue = await self.get_queue(project, location, queue_name)
        task = CloudTask(
            queue_full_name=queue.full_name,
            task_id=str(uuid.uuid4()),
            payload=json.dumps(payload),
            state="pending",
            created_at=_now(),
        )
        saved = await self._tasks.save(task)
        await self._telemetry.emit(
            "gcp", "cloudtasks", "create_task", {"queue": queue_name}
        )
        return saved

    async def list_tasks(
        self, project: str, location: str, queue_name: str
    ) -> list[CloudTask]:
        queue = await self.get_queue(project, location, queue_name)
        return await self._tasks.list_pending(queue.full_name)

    async def delete_task(self, task_id: str) -> None:
        await self._tasks.delete(task_id)
