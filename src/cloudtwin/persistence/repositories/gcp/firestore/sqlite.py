"""GCP Firestore — SQLite repository implementations."""

from __future__ import annotations

import json
from datetime import datetime, timezone
from typing import Optional

from cloudtwin.persistence.models.gcp.firestore import FirestoreDocument
from cloudtwin.persistence.repositories.gcp.firestore.repository import (
    FirestoreDocumentRepository,
)

DDL = """
CREATE TABLE IF NOT EXISTS firestore_documents (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    project     TEXT    NOT NULL,
    collection  TEXT    NOT NULL,
    document_id TEXT    NOT NULL,
    fields      TEXT    NOT NULL DEFAULT '{}',
    created_at  TEXT    NOT NULL,
    UNIQUE(project, collection, document_id)
);
"""


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class SqliteFirestoreDocumentRepository(FirestoreDocumentRepository):
    def __init__(self, db):
        self._db = db

    def _row(self, row) -> FirestoreDocument:
        return FirestoreDocument(
            id=row["id"],
            project=row["project"],
            collection=row["collection"],
            document_id=row["document_id"],
            fields=row["fields"],
            created_at=row["created_at"],
        )

    async def get(
        self, project: str, collection: str, document_id: str
    ) -> Optional[FirestoreDocument]:
        async with self._db.conn.execute(
            "SELECT * FROM firestore_documents WHERE project=? AND collection=? AND document_id=?",
            (project, collection, document_id),
        ) as cur:
            row = await cur.fetchone()
            return self._row(row) if row else None

    async def list_by_collection(
        self, project: str, collection: str
    ) -> list[FirestoreDocument]:
        async with self._db.conn.execute(
            "SELECT * FROM firestore_documents WHERE project=? AND collection=? ORDER BY id",
            (project, collection),
        ) as cur:
            return [self._row(r) for r in await cur.fetchall()]

    async def save(self, doc: FirestoreDocument) -> FirestoreDocument:
        await self._db.conn.execute(
            "INSERT OR REPLACE INTO firestore_documents (project, collection, document_id, fields, created_at) VALUES (?,?,?,?,?)",
            (
                doc.project,
                doc.collection,
                doc.document_id,
                doc.fields,
                doc.created_at or _now(),
            ),
        )
        await self._db.conn.commit()
        return await self.get(doc.project, doc.collection, doc.document_id)

    async def delete(self, project: str, collection: str, document_id: str) -> None:
        await self._db.conn.execute(
            "DELETE FROM firestore_documents WHERE project=? AND collection=? AND document_id=?",
            (project, collection, document_id),
        )
        await self._db.conn.commit()

    async def query(
        self, project: str, collection: str, field: str, op: str, value: str
    ) -> list[FirestoreDocument]:
        docs = await self.list_by_collection(project, collection)
        results = []
        for doc in docs:
            try:
                fields = json.loads(doc.fields)
                raw = fields.get(field, "")
                # Support Firestore typed values: {"stringValue": "...", "integerValue": ...}
                if isinstance(raw, dict):
                    doc_val = str(
                        raw.get("stringValue")
                        or raw.get("integerValue")
                        or raw.get("doubleValue")
                        or raw.get("booleanValue")
                        or ""
                    )
                else:
                    doc_val = str(raw)
                if op == "==" and doc_val == value:
                    results.append(doc)
                elif op == "!=" and doc_val != value:
                    results.append(doc)
            except (ValueError, TypeError):
                pass
        return results
