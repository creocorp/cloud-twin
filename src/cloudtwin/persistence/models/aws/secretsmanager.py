"""AWS Secrets Manager domain model dataclasses."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional


@dataclass
class Secret:
    name: str
    arn: str
    created_at: str
    id: Optional[int] = None


@dataclass
class SecretVersion:
    secret_name: str
    version_id: str
    secret_string: Optional[str]
    secret_binary: Optional[bytes]
    created_at: str
    id: Optional[int] = None
