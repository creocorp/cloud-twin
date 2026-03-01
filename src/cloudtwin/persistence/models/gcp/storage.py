"""GCP Cloud Storage domain model dataclasses."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional


@dataclass
class GcsBucket:
    project: str
    name: str
    location: str
    created_at: str
    id: Optional[int] = None


@dataclass
class GcsObject:
    bucket_id: int
    name: str
    content_type: Optional[str]
    content_length: Optional[int]
    data: Optional[bytes]
    metadata: str  # JSON dict string
    created_at: str
    id: Optional[int] = None
