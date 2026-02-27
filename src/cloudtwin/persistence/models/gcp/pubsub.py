"""GCP Pub/Sub domain model dataclasses."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional


@dataclass
class PubsubTopic:
    project: str
    name: str                 # short name only (no "projects/..." prefix)
    full_name: str            # "projects/{project}/topics/{name}"
    created_at: str
    id: Optional[int] = None


@dataclass
class PubsubSubscription:
    project: str
    name: str                 # short name only
    full_name: str            # "projects/{project}/subscriptions/{name}"
    topic_full_name: str
    ack_deadline_seconds: int
    created_at: str
    id: Optional[int] = None


@dataclass
class PubsubMessage:
    message_id: str
    topic_full_name: str
    data: str                 # base64-encoded payload
    attributes: str           # JSON dict string
    created_at: str
    id: Optional[int] = None


@dataclass
class PubsubAckable:
    """A delivered-but-not-yet-acked message for a subscription."""
    ack_id: str
    message_id: str
    subscription_full_name: str
    delivery_attempt: int
    ack_deadline_seconds: int
    created_at: str
    id: Optional[int] = None
