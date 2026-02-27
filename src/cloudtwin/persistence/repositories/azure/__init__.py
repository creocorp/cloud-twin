"""Azure repositories — public re-exports."""

from __future__ import annotations

from cloudtwin.persistence.repositories.azure.blob import (
    AzureContainerRepository,
    AzureBlobRepository,
    InMemoryAzureContainerRepository,
    InMemoryAzureBlobRepository,
    SqliteAzureContainerRepository,
    SqliteAzureBlobRepository,
)
from cloudtwin.persistence.repositories.azure.servicebus import (
    AsbQueueRepository,
    AsbTopicRepository,
    AsbSubscriptionRepository,
    AsbMessageRepository,
    InMemoryAsbQueueRepository,
    InMemoryAsbTopicRepository,
    InMemoryAsbSubscriptionRepository,
    InMemoryAsbMessageRepository,
    SqliteAsbQueueRepository,
    SqliteAsbTopicRepository,
    SqliteAsbSubscriptionRepository,
    SqliteAsbMessageRepository,
)

__all__ = [
    # Blob
    "AzureContainerRepository", "AzureBlobRepository",
    "InMemoryAzureContainerRepository", "InMemoryAzureBlobRepository",
    "SqliteAzureContainerRepository", "SqliteAzureBlobRepository",
    # Service Bus
    "AsbQueueRepository", "AsbTopicRepository", "AsbSubscriptionRepository", "AsbMessageRepository",
    "InMemoryAsbQueueRepository", "InMemoryAsbTopicRepository",
    "InMemoryAsbSubscriptionRepository", "InMemoryAsbMessageRepository",
    "SqliteAsbQueueRepository", "SqliteAsbTopicRepository",
    "SqliteAsbSubscriptionRepository", "SqliteAsbMessageRepository",
]
