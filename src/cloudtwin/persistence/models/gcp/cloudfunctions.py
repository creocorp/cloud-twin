"""GCP Cloud Functions domain model dataclasses."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional


@dataclass
class CloudFunction:
    project: str
    name: str
    full_name: str
    runtime: str
    entry_point: str
    source_code: str
    created_at: str
    id: Optional[int] = None


@dataclass
class CloudFunctionInvocation:
    function_full_name: str
    invocation_id: str
    payload: str
    response: str
    created_at: str
    id: Optional[int] = None
