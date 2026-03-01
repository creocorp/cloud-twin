"""SNS repositories — public re-exports."""

from __future__ import annotations

from cloudtwin.persistence.repositories.aws.sns.inmemory import (
    InMemorySnsMessageRepository,
    InMemorySnsSubscriptionRepository,
    InMemorySnsTopicRepository,
)
from cloudtwin.persistence.repositories.aws.sns.repository import (
    SnsMessageRepository,
    SnsSubscriptionRepository,
    SnsTopicRepository,
)
from cloudtwin.persistence.repositories.aws.sns.sqlite import (
    SqliteSnsMessageRepository,
    SqliteSnsSubscriptionRepository,
    SqliteSnsTopicRepository,
)

__all__ = [
    "SnsTopicRepository",
    "SnsSubscriptionRepository",
    "SnsMessageRepository",
    "InMemorySnsTopicRepository",
    "InMemorySnsSubscriptionRepository",
    "InMemorySnsMessageRepository",
    "SqliteSnsTopicRepository",
    "SqliteSnsSubscriptionRepository",
    "SqliteSnsMessageRepository",
]
