"""SQS repositories — public re-exports."""

from __future__ import annotations

from cloudtwin.persistence.repositories.aws.sqs.repository import (
    SqsQueueRepository,
    SqsMessageRepository,
)
from cloudtwin.persistence.repositories.aws.sqs.inmemory import (
    InMemorySqsQueueRepository,
    InMemorySqsMessageRepository,
)
from cloudtwin.persistence.repositories.aws.sqs.sqlite import (
    SqliteSqsQueueRepository,
    SqliteSqsMessageRepository,
)

__all__ = [
    "SqsQueueRepository",
    "SqsMessageRepository",
    "InMemorySqsQueueRepository",
    "InMemorySqsMessageRepository",
    "SqliteSqsQueueRepository",
    "SqliteSqsMessageRepository",
]
