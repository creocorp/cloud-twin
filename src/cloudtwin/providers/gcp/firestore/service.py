"""GCP Firestore — pure business logic."""

from __future__ import annotations

import json
import logging
import uuid
from datetime import datetime, timezone

from cloudtwin.core.errors import NotFoundError
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.persistence.models.gcp.firestore import FirestoreDocument
from cloudtwin.persistence.repositories.gcp.firestore import FirestoreDocumentRepository

log = logging.getLogger("cloudtwin.gcp.firestore")


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class FirestoreService:
    def __init__(self, repo: FirestoreDocumentRepository, telemetry: TelemetryEngine):
        self._repo = repo
        self._telemetry = telemetry

    async def set_document(
        self, project: str, collection: str, document_id: str, fields: dict
    ) -> FirestoreDocument:
        doc = FirestoreDocument(
            project=project,
            collection=collection,
            document_id=document_id,
            fields=json.dumps(fields),
            created_at=_now(),
        )
        saved = await self._repo.save(doc)
        await self._telemetry.emit(
            "gcp",
            "firestore",
            "set_document",
            {"project": project, "collection": collection},
        )
        return saved

    async def get_document(
        self, project: str, collection: str, document_id: str
    ) -> FirestoreDocument:
        doc = await self._repo.get(project, collection, document_id)
        if not doc:
            raise NotFoundError(f"Document not found: {collection}/{document_id}")
        return doc

    async def delete_document(
        self, project: str, collection: str, document_id: str
    ) -> None:
        await self._repo.delete(project, collection, document_id)
        await self._telemetry.emit(
            "gcp",
            "firestore",
            "delete_document",
            {"project": project, "collection": collection},
        )

    async def list_documents(
        self, project: str, collection: str
    ) -> list[FirestoreDocument]:
        return await self._repo.list_by_collection(project, collection)

    async def query(
        self, project: str, collection: str, field: str, op: str, value: str
    ) -> list[FirestoreDocument]:
        return await self._repo.query(project, collection, field, op, value)
