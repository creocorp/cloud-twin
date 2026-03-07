"""GCP Firestore — repository package re-exports."""

from __future__ import annotations

from cloudtwin.persistence.repositories.gcp.firestore.repository import (
    FirestoreDocumentRepository,
)
from cloudtwin.persistence.repositories.gcp.firestore.sqlite import (
    SqliteFirestoreDocumentRepository,
)

__all__ = [
    "FirestoreDocumentRepository",
    "SqliteFirestoreDocumentRepository",
]
