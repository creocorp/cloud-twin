"""GCP Firestore — abstract repository interfaces."""

from __future__ import annotations

from abc import ABC, abstractmethod
from typing import Optional

from cloudtwin.persistence.models.gcp.firestore import FirestoreDocument


class FirestoreDocumentRepository(ABC):
    @abstractmethod
    async def get(
        self, project: str, collection: str, document_id: str
    ) -> Optional[FirestoreDocument]: ...
    @abstractmethod
    async def list_by_collection(
        self, project: str, collection: str
    ) -> list[FirestoreDocument]: ...
    @abstractmethod
    async def save(self, doc: FirestoreDocument) -> FirestoreDocument: ...
    @abstractmethod
    async def delete(self, project: str, collection: str, document_id: str) -> None: ...
    @abstractmethod
    async def query(
        self, project: str, collection: str, field: str, op: str, value: str
    ) -> list[FirestoreDocument]: ...
