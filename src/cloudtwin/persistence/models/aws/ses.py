"""SES domain model dataclasses."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional


@dataclass
class SesIdentity:
    identity: str
    type: str          # "domain" | "email"
    verified: bool
    token: Optional[str]
    created_at: str
    id: Optional[int] = None


@dataclass
class SesMessage:
    message_id: str
    source: str
    destinations: list[str]
    subject: Optional[str]
    text_body: Optional[str]
    html_body: Optional[str]
    raw_mime: Optional[bytes]
    status: str               # "sent" | "failed"
    error_message: Optional[str]
    created_at: str
    id: Optional[int] = None
