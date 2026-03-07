"""AWS DynamoDB — repository package re-exports."""

from __future__ import annotations

from cloudtwin.persistence.repositories.aws.dynamodb.repository import (
    DynamoItemRepository,
    DynamoTableRepository,
)
from cloudtwin.persistence.repositories.aws.dynamodb.sqlite import (
    SqliteDynamoItemRepository,
    SqliteDynamoTableRepository,
)

__all__ = [
    "DynamoTableRepository",
    "DynamoItemRepository",
    "SqliteDynamoTableRepository",
    "SqliteDynamoItemRepository",
]
