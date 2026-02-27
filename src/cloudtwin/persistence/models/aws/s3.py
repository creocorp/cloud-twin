"""S3 domain model dataclasses."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional


@dataclass
class S3Bucket:
    name: str
    created_at: str
    id: Optional[int] = None


@dataclass
class S3Object:
    bucket_id: int
    key: str
    content_type: Optional[str]
    content_length: Optional[int]
    data: Optional[bytes]
    created_at: str
    id: Optional[int] = None
