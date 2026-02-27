"""SNS domain model dataclasses."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional


@dataclass
class SnsTopic:
    name: str
    arn: str
    created_at: str
    id: Optional[int] = None


@dataclass
class SnsSubscription:
    subscription_arn: str
    topic_arn: str
    protocol: str
    endpoint: str
    created_at: str
    id: Optional[int] = None


@dataclass
class SnsMessage:
    message_id: str
    topic_arn: str
    message: str
    subject: Optional[str]
    created_at: str
    id: Optional[int] = None
