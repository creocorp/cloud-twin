"""Azure Queue Storage — repository package re-exports."""

from __future__ import annotations

from cloudtwin.persistence.repositories.azure.queue.repository import (
    AzureQueueMessageRepository,
    AzureStorageQueueRepository,
)
from cloudtwin.persistence.repositories.azure.queue.sqlite import (
    SqliteAzureQueueMessageRepository,
    SqliteAzureStorageQueueRepository,
)

__all__ = [
    "AzureStorageQueueRepository",
    "AzureQueueMessageRepository",
    "SqliteAzureStorageQueueRepository",
    "SqliteAzureQueueMessageRepository",
]
