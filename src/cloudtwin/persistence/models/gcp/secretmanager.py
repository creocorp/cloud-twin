"""GCP Secret Manager domain model dataclasses."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional


@dataclass
class GcpSecret:
    project: str
    name: str
    full_name: str
    created_at: str
    id: Optional[int] = None


@dataclass
class GcpSecretVersion:
    secret_full_name: str
    version_id: str
    payload: str     # base64-encoded secret data
    state: str       # "enabled" | "disabled" | "destroyed"
    created_at: str
    id: Optional[int] = None
