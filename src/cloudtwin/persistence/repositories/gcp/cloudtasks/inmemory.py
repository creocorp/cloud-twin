"""GCP Cloud Tasks — in-memory repository implementations."""

from __future__ import annotations

from typing import Optional

from cloudtwin.persistence.models.gcp.cloudtasks import CloudTask, CloudTasksQueue
from cloudtwin.persistence.repositories.gcp.cloudtasks.repository import (
    CloudTaskRepository,
    CloudTasksQueueRepository,
)


class InMemoryCloudTasksQueueRepository(CloudTasksQueueRepository):
    def __init__(self):
        self._store: dict[str, CloudTasksQueue] = {}
        self._next_id = 1

    async def get(self, full_name: str) -> Optional[CloudTasksQueue]:
        return self._store.get(full_name)

    async def list_by_project(self, project: str) -> list[CloudTasksQueue]:
        return [q for q in self._store.values() if q.project == project]

    async def save(self, queue: CloudTasksQueue) -> CloudTasksQueue:
        if queue.full_name not in self._store:
            queue.id = self._next_id
            self._next_id += 1
        self._store[queue.full_name] = queue
        return queue

    async def delete(self, full_name: str) -> None:
        self._store.pop(full_name, None)


class InMemoryCloudTaskRepository(CloudTaskRepository):
    def __init__(self):
        self._store: dict[str, CloudTask] = {}
        self._next_id = 1

    async def save(self, task: CloudTask) -> CloudTask:
        if task.task_id not in self._store:
            task.id = self._next_id
            self._next_id += 1
        self._store[task.task_id] = task
        return task

    async def get(self, queue_full_name: str, task_id: str) -> Optional[CloudTask]:
        t = self._store.get(task_id)
        return t if t and t.queue_full_name == queue_full_name else None

    async def list_pending(self, queue_full_name: str, limit: int = 10) -> list[CloudTask]:
        results = [t for t in self._store.values()
                   if t.queue_full_name == queue_full_name and t.state == "pending"]
        return sorted(results, key=lambda t: t.id or 0)[:limit]

    async def update_state(self, task_id: str, state: str) -> None:
        if task_id in self._store:
            self._store[task_id].state = state

    async def delete(self, task_id: str) -> None:
        self._store.pop(task_id, None)
