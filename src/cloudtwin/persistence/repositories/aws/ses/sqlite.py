"""SES — SQLite repository implementations."""

from __future__ import annotations

import json
from datetime import datetime, timezone
from typing import Optional

from cloudtwin.persistence.models.aws.ses import SesIdentity, SesMessage
from cloudtwin.persistence.repositories.aws.ses.repository import (
    SesIdentityRepository,
    SesMessageRepository,
)

DDL = """
CREATE TABLE IF NOT EXISTS ses_identities (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    identity    TEXT    NOT NULL UNIQUE,
    type        TEXT    NOT NULL,  -- 'domain' or 'email'
    verified    INTEGER NOT NULL DEFAULT 0,
    token       TEXT,
    created_at  TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS ses_messages (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id    TEXT    NOT NULL UNIQUE,
    source        TEXT    NOT NULL,
    destinations  TEXT    NOT NULL,  -- JSON array
    subject       TEXT,
    text_body     TEXT,
    html_body     TEXT,
    raw_mime      BLOB,
    status        TEXT    NOT NULL DEFAULT 'sent',
    error_message TEXT,
    created_at    TEXT    NOT NULL
);
"""


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class SqliteSesIdentityRepository(SesIdentityRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> SesIdentity:
        return SesIdentity(
            id=row["id"], identity=row["identity"], type=row["type"],
            verified=bool(row["verified"]), token=row["token"], created_at=row["created_at"],
        )

    async def get(self, identity: str) -> Optional[SesIdentity]:
        async with self._db.conn.execute(
            "SELECT * FROM ses_identities WHERE identity = ?", (identity,)
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def list_all(self) -> list[SesIdentity]:
        async with self._db.conn.execute("SELECT * FROM ses_identities") as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def save(self, identity: SesIdentity) -> SesIdentity:
        await self._db.conn.execute(
            """
            INSERT INTO ses_identities (identity, type, verified, token, created_at)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(identity) DO UPDATE SET verified = excluded.verified, token = excluded.token
            """,
            (identity.identity, identity.type, int(identity.verified),
             identity.token, identity.created_at or _now()),
        )
        await self._db.conn.commit()
        return await self.get(identity.identity)

    async def delete(self, identity: str) -> None:
        await self._db.conn.execute("DELETE FROM ses_identities WHERE identity = ?", (identity,))
        await self._db.conn.commit()


class SqliteSesMessageRepository(SesMessageRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> SesMessage:
        return SesMessage(
            id=row["id"], message_id=row["message_id"], source=row["source"],
            destinations=json.loads(row["destinations"]), subject=row["subject"],
            text_body=row["text_body"], html_body=row["html_body"],
            raw_mime=row["raw_mime"], status=row["status"],
            error_message=row["error_message"], created_at=row["created_at"],
        )

    async def save(self, message: SesMessage) -> SesMessage:
        await self._db.conn.execute(
            """
            INSERT INTO ses_messages
                (message_id, source, destinations, subject, text_body, html_body,
                 raw_mime, status, error_message, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (message.message_id, message.source, json.dumps(message.destinations),
             message.subject, message.text_body, message.html_body,
             message.raw_mime, message.status, message.error_message,
             message.created_at or _now()),
        )
        await self._db.conn.commit()
        return message

    async def list_all(self) -> list[SesMessage]:
        async with self._db.conn.execute("SELECT * FROM ses_messages ORDER BY id DESC") as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def get(self, message_id: str) -> Optional[SesMessage]:
        async with self._db.conn.execute(
            "SELECT * FROM ses_messages WHERE message_id = ?", (message_id,)
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None
