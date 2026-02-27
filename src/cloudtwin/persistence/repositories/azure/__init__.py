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
from cloudtwin.persistence.repositories.azure.queue import (
    AzureStorageQueueRepository,
    AzureQueueMessageRepository,
    InMemoryAzureStorageQueueRepository,
    InMemoryAzureQueueMessageRepository,
    SqliteAzureStorageQueueRepository,
    SqliteAzureQueueMessageRepository,
)
from cloudtwin.persistence.repositories.azure.keyvault import (
    KeyVaultSecretRepository,
    InMemoryKeyVaultSecretRepository,
    SqliteKeyVaultSecretRepository,
)
from cloudtwin.persistence.repositories.azure.eventgrid import (
    EventGridTopicRepository,
    EventGridEventRepository,
    InMemoryEventGridTopicRepository,
    InMemoryEventGridEventRepository,
    SqliteEventGridTopicRepository,
    SqliteEventGridEventRepository,
)
from cloudtwin.persistence.repositories.azure.functions import (
    AzureFunctionRepository,
    AzureFunctionInvocationRepository,
    InMemoryAzureFunctionRepository,
    InMemoryAzureFunctionInvocationRepository,
    SqliteAzureFunctionRepository,
    SqliteAzureFunctionInvocationRepository,
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
    # Queue Storage
    "AzureStorageQueueRepository", "AzureQueueMessageRepository",
    "InMemoryAzureStorageQueueRepository", "InMemoryAzureQueueMessageRepository",
    "SqliteAzureStorageQueueRepository", "SqliteAzureQueueMessageRepository",
    # Key Vault
    "KeyVaultSecretRepository",
    "InMemoryKeyVaultSecretRepository",
    "SqliteKeyVaultSecretRepository",
    # Event Grid
    "EventGridTopicRepository", "EventGridEventRepository",
    "InMemoryEventGridTopicRepository", "InMemoryEventGridEventRepository",
    "SqliteEventGridTopicRepository", "SqliteEventGridEventRepository",
    # Functions
    "AzureFunctionRepository", "AzureFunctionInvocationRepository",
    "InMemoryAzureFunctionRepository", "InMemoryAzureFunctionInvocationRepository",
    "SqliteAzureFunctionRepository", "SqliteAzureFunctionInvocationRepository",
]
