"""GCP Cloud Tasks — abstract repository interfaces."""

from __future__ import annotations

from abc import ABC, abstractmethod
from typing import Optional

from cloudtwin.persistence.models.gcp.cloudtasks import CloudTask, CloudTasksQueue


class CloudTasksQueueRepository(ABC):
    @abstractmethod
    async def get(self, full_name: str) -> Optional[CloudTasksQueue]: ...
    @abstractmethod
    async def list_by_project(self, project: str) -> list[CloudTasksQueue]: ...
    @abstractmethod
    async def save(self, queue: CloudTasksQueue) -> CloudTasksQueue: ...
    @abstractmethod
    async def delete(self, full_name: str) -> None: ...


class CloudTaskRepository(ABC):
    @abstractmethod
    async def save(self, task: CloudTask) -> CloudTask: ...
    @abstractmethod
    async def get(self, queue_full_name: str, task_id: str) -> Optional[CloudTask]: ...
    @abstractmethod
    async def list_pending(
        self, queue_full_name: str, limit: int = 10
    ) -> list[CloudTask]: ...
    @abstractmethod
    async def update_state(self, task_id: str, state: str) -> None: ...
    @abstractmethod
    async def delete(self, task_id: str) -> None: ...
