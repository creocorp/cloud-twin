"""Azure Key Vault domain model dataclasses."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional


@dataclass
class KeyVaultSecret:
    vault: str
    name: str
    value: str
    version: str
    created_at: str
    id: Optional[int] = None
