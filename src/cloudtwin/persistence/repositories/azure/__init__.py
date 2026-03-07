"""Azure repositories — public re-exports."""

from __future__ import annotations

from cloudtwin.persistence.repositories.azure.blob import (
    AzureBlobRepository,
    AzureContainerRepository,
    SqliteAzureBlobRepository,
    SqliteAzureContainerRepository,
)
from cloudtwin.persistence.repositories.azure.eventgrid import (
    EventGridEventRepository,
    EventGridTopicRepository,
    SqliteEventGridEventRepository,
    SqliteEventGridTopicRepository,
)
from cloudtwin.persistence.repositories.azure.functions import (
    AzureFunctionInvocationRepository,
    AzureFunctionRepository,
    SqliteAzureFunctionInvocationRepository,
    SqliteAzureFunctionRepository,
)
from cloudtwin.persistence.repositories.azure.keyvault import (
    KeyVaultSecretRepository,
    SqliteKeyVaultSecretRepository,
)
from cloudtwin.persistence.repositories.azure.queue import (
    AzureQueueMessageRepository,
    AzureStorageQueueRepository,
    SqliteAzureQueueMessageRepository,
    SqliteAzureStorageQueueRepository,
)
from cloudtwin.persistence.repositories.azure.servicebus import (
    AsbMessageRepository,
    AsbQueueRepository,
    AsbSubscriptionRepository,
    AsbTopicRepository,
    SqliteAsbMessageRepository,
    SqliteAsbQueueRepository,
    SqliteAsbSubscriptionRepository,
    SqliteAsbTopicRepository,
)

__all__ = [
    # Blob
    "AzureContainerRepository",
    "AzureBlobRepository",
    "SqliteAzureContainerRepository",
    "SqliteAzureBlobRepository",
    # Service Bus
    "AsbQueueRepository",
    "AsbTopicRepository",
    "AsbSubscriptionRepository",
    "AsbMessageRepository",
    "SqliteAsbQueueRepository",
    "SqliteAsbTopicRepository",
    "SqliteAsbSubscriptionRepository",
    "SqliteAsbMessageRepository",
    # Queue Storage
    "AzureStorageQueueRepository",
    "AzureQueueMessageRepository",
    "SqliteAzureStorageQueueRepository",
    "SqliteAzureQueueMessageRepository",
    # Key Vault
    "KeyVaultSecretRepository",
    "SqliteKeyVaultSecretRepository",
    # Event Grid
    "EventGridTopicRepository",
    "EventGridEventRepository",
    "SqliteEventGridTopicRepository",
    "SqliteEventGridEventRepository",
    # Functions
    "AzureFunctionRepository",
    "AzureFunctionInvocationRepository",
    "SqliteAzureFunctionRepository",
    "SqliteAzureFunctionInvocationRepository",
]
