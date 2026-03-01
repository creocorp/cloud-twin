"""SNS — SQLite repository implementations."""

from __future__ import annotations

from datetime import datetime, timezone
from typing import Optional

from cloudtwin.persistence.models.aws.sns import SnsMessage, SnsSubscription, SnsTopic
from cloudtwin.persistence.repositories.aws.sns.repository import (
    SnsMessageRepository,
    SnsSubscriptionRepository,
    SnsTopicRepository,
)

DDL = """
CREATE TABLE IF NOT EXISTS sns_topics (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT    NOT NULL UNIQUE,
    arn        TEXT    NOT NULL UNIQUE,
    created_at TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS sns_subscriptions (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    subscription_arn TEXT    NOT NULL UNIQUE,
    topic_arn        TEXT    NOT NULL,
    protocol         TEXT    NOT NULL,
    endpoint         TEXT    NOT NULL,
    created_at       TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS sns_messages (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id TEXT    NOT NULL UNIQUE,
    topic_arn  TEXT    NOT NULL,
    message    TEXT    NOT NULL,
    subject    TEXT,
    created_at TEXT    NOT NULL
);
"""


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class SqliteSnsTopicRepository(SnsTopicRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> SnsTopic:
        return SnsTopic(
            id=row["id"], name=row["name"], arn=row["arn"], created_at=row["created_at"]
        )

    async def get(self, arn: str) -> Optional[SnsTopic]:
        async with self._db.conn.execute(
            "SELECT * FROM sns_topics WHERE arn = ?", (arn,)
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def get_by_name(self, name: str) -> Optional[SnsTopic]:
        async with self._db.conn.execute(
            "SELECT * FROM sns_topics WHERE name = ?", (name,)
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def list_all(self) -> list[SnsTopic]:
        async with self._db.conn.execute("SELECT * FROM sns_topics") as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def save(self, topic: SnsTopic) -> SnsTopic:
        await self._db.conn.execute(
            "INSERT OR IGNORE INTO sns_topics (name, arn, created_at) VALUES (?, ?, ?)",
            (topic.name, topic.arn, topic.created_at or _now()),
        )
        await self._db.conn.commit()
        return await self.get(topic.arn)

    async def delete(self, arn: str) -> None:
        await self._db.conn.execute("DELETE FROM sns_topics WHERE arn = ?", (arn,))
        await self._db.conn.commit()


class SqliteSnsSubscriptionRepository(SnsSubscriptionRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> SnsSubscription:
        return SnsSubscription(
            id=row["id"],
            subscription_arn=row["subscription_arn"],
            topic_arn=row["topic_arn"],
            protocol=row["protocol"],
            endpoint=row["endpoint"],
            created_at=row["created_at"],
        )

    async def get(self, subscription_arn: str) -> Optional[SnsSubscription]:
        async with self._db.conn.execute(
            "SELECT * FROM sns_subscriptions WHERE subscription_arn = ?",
            (subscription_arn,),
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def list_by_topic(self, topic_arn: str) -> list[SnsSubscription]:
        async with self._db.conn.execute(
            "SELECT * FROM sns_subscriptions WHERE topic_arn = ?", (topic_arn,)
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def save(self, sub: SnsSubscription) -> SnsSubscription:
        await self._db.conn.execute(
            """
            INSERT OR IGNORE INTO sns_subscriptions
                (subscription_arn, topic_arn, protocol, endpoint, created_at)
            VALUES (?, ?, ?, ?, ?)
            """,
            (
                sub.subscription_arn,
                sub.topic_arn,
                sub.protocol,
                sub.endpoint,
                sub.created_at or _now(),
            ),
        )
        await self._db.conn.commit()
        return await self.get(sub.subscription_arn)

    async def list_all(self) -> list[SnsSubscription]:
        async with self._db.conn.execute("SELECT * FROM sns_subscriptions") as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def delete(self, subscription_arn: str) -> None:
        await self._db.conn.execute(
            "DELETE FROM sns_subscriptions WHERE subscription_arn = ?",
            (subscription_arn,),
        )
        await self._db.conn.commit()


class SqliteSnsMessageRepository(SnsMessageRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> SnsMessage:
        return SnsMessage(
            id=row["id"],
            message_id=row["message_id"],
            topic_arn=row["topic_arn"],
            message=row["message"],
            subject=row["subject"],
            created_at=row["created_at"],
        )

    async def save(self, message: SnsMessage) -> SnsMessage:
        await self._db.conn.execute(
            "INSERT INTO sns_messages (message_id, topic_arn, message, subject, created_at)"
            " VALUES (?, ?, ?, ?, ?)",
            (
                message.message_id,
                message.topic_arn,
                message.message,
                message.subject,
                message.created_at or _now(),
            ),
        )
        await self._db.conn.commit()
        return message

    async def list_by_topic(self, topic_arn: str) -> list[SnsMessage]:
        async with self._db.conn.execute(
            "SELECT * FROM sns_messages WHERE topic_arn = ? ORDER BY id DESC",
            (topic_arn,),
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]
