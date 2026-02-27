"""SNS repositories — public re-exports."""

from __future__ import annotations

from cloudtwin.persistence.repositories.aws.sns.repository import (
    SnsTopicRepository,
    SnsSubscriptionRepository,
    SnsMessageRepository,
)
from cloudtwin.persistence.repositories.aws.sns.inmemory import (
    InMemorySnsTopicRepository,
    InMemorySnsSubscriptionRepository,
    InMemorySnsMessageRepository,
)
from cloudtwin.persistence.repositories.aws.sns.sqlite import (
    SqliteSnsTopicRepository,
    SqliteSnsSubscriptionRepository,
    SqliteSnsMessageRepository,
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
