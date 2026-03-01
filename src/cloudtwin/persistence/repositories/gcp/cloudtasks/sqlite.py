"""GCP Cloud Tasks — SQLite repository implementations."""

from __future__ import annotations

from datetime import datetime, timezone
from typing import Optional

from cloudtwin.persistence.models.gcp.cloudtasks import CloudTask, CloudTasksQueue
from cloudtwin.persistence.repositories.gcp.cloudtasks.repository import (
    CloudTaskRepository,
    CloudTasksQueueRepository,
)

DDL = """
CREATE TABLE IF NOT EXISTS cloudtasks_queues (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    project    TEXT    NOT NULL,
    location   TEXT    NOT NULL,
    name       TEXT    NOT NULL,
    full_name  TEXT    NOT NULL UNIQUE,
    created_at TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS cloudtasks_tasks (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    queue_full_name TEXT    NOT NULL,
    task_id         TEXT    NOT NULL UNIQUE,
    payload         TEXT    NOT NULL DEFAULT '{}',
    state           TEXT    NOT NULL DEFAULT 'pending',
    created_at      TEXT    NOT NULL
);
"""


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class SqliteCloudTasksQueueRepository(CloudTasksQueueRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> CloudTasksQueue:
        return CloudTasksQueue(
            id=row["id"],
            project=row["project"],
            location=row["location"],
            name=row["name"],
            full_name=row["full_name"],
            created_at=row["created_at"],
        )

    async def get(self, full_name: str) -> Optional[CloudTasksQueue]:
        async with self._db.conn.execute(
            "SELECT * FROM cloudtasks_queues WHERE full_name = ?", (full_name,)
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def list_by_project(self, project: str) -> list[CloudTasksQueue]:
        async with self._db.conn.execute(
            "SELECT * FROM cloudtasks_queues WHERE project = ? ORDER BY id", (project,)
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def save(self, queue: CloudTasksQueue) -> CloudTasksQueue:
        await self._db.conn.execute(
            "INSERT OR IGNORE INTO cloudtasks_queues (project, location, name, full_name, created_at) VALUES (?,?,?,?,?)",
            (
                queue.project,
                queue.location,
                queue.name,
                queue.full_name,
                queue.created_at or _now(),
            ),
        )
        await self._db.conn.commit()
        return await self.get(queue.full_name)

    async def delete(self, full_name: str) -> None:
        await self._db.conn.execute(
            "DELETE FROM cloudtasks_queues WHERE full_name = ?", (full_name,)
        )
        await self._db.conn.commit()


class SqliteCloudTaskRepository(CloudTaskRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> CloudTask:
        return CloudTask(
            id=row["id"],
            queue_full_name=row["queue_full_name"],
            task_id=row["task_id"],
            payload=row["payload"],
            state=row["state"],
            created_at=row["created_at"],
        )

    async def save(self, task: CloudTask) -> CloudTask:
        await self._db.conn.execute(
            "INSERT OR IGNORE INTO cloudtasks_tasks (queue_full_name, task_id, payload, state, created_at) VALUES (?,?,?,?,?)",
            (
                task.queue_full_name,
                task.task_id,
                task.payload,
                task.state,
                task.created_at or _now(),
            ),
        )
        await self._db.conn.commit()
        return task

    async def get(self, queue_full_name: str, task_id: str) -> Optional[CloudTask]:
        async with self._db.conn.execute(
            "SELECT * FROM cloudtasks_tasks WHERE queue_full_name = ? AND task_id = ?",
            (queue_full_name, task_id),
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def list_pending(
        self, queue_full_name: str, limit: int = 10
    ) -> list[CloudTask]:
        async with self._db.conn.execute(
            "SELECT * FROM cloudtasks_tasks WHERE queue_full_name = ? AND state = 'pending' ORDER BY id LIMIT ?",
            (queue_full_name, limit),
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def update_state(self, task_id: str, state: str) -> None:
        await self._db.conn.execute(
            "UPDATE cloudtasks_tasks SET state = ? WHERE task_id = ?", (state, task_id)
        )
        await self._db.conn.commit()

    async def delete(self, task_id: str) -> None:
        await self._db.conn.execute(
            "DELETE FROM cloudtasks_tasks WHERE task_id = ?", (task_id,)
        )
        await self._db.conn.commit()
