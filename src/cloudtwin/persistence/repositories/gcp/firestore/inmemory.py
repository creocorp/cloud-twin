"""GCP Firestore — in-memory repository implementations."""

from __future__ import annotations

import json
from typing import Optional

from cloudtwin.persistence.models.gcp.firestore import FirestoreDocument
from cloudtwin.persistence.repositories.gcp.firestore.repository import (
    FirestoreDocumentRepository,
)


class InMemoryFirestoreDocumentRepository(FirestoreDocumentRepository):
    def __init__(self):
        self._store: dict[tuple[str, str, str], FirestoreDocument] = {}
        self._next_id = 1

    def _key(self, project: str, collection: str, document_id: str):
        return (project, collection, document_id)

    async def get(
        self, project: str, collection: str, document_id: str
    ) -> Optional[FirestoreDocument]:
        return self._store.get(self._key(project, collection, document_id))

    async def list_by_collection(
        self, project: str, collection: str
    ) -> list[FirestoreDocument]:
        return [
            d
            for d in self._store.values()
            if d.project == project and d.collection == collection
        ]

    async def save(self, doc: FirestoreDocument) -> FirestoreDocument:
        k = self._key(doc.project, doc.collection, doc.document_id)
        if k not in self._store:
            doc.id = self._next_id
            self._next_id += 1
        self._store[k] = doc
        return doc

    async def delete(self, project: str, collection: str, document_id: str) -> None:
        self._store.pop(self._key(project, collection, document_id), None)

    async def query(
        self, project: str, collection: str, field: str, op: str, value: str
    ) -> list[FirestoreDocument]:
        results = []
        for doc in self._store.values():
            if doc.project != project or doc.collection != collection:
                continue
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
