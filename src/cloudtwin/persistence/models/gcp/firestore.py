"""GCP Firestore domain model dataclasses."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional


@dataclass
class FirestoreDocument:
    project: str
    collection: str
    document_id: str
    fields: str  # JSON blob
    created_at: str
    id: Optional[int] = None
