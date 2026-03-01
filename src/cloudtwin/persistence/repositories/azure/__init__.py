"""Azure repositories — public re-exports."""

from __future__ import annotations

from cloudtwin.persistence.repositories.azure.blob import (
    AzureBlobRepository,
    AzureContainerRepository,
    InMemoryAzureBlobRepository,
    InMemoryAzureContainerRepository,
    SqliteAzureBlobRepository,
    SqliteAzureContainerRepository,
)
from cloudtwin.persistence.repositories.azure.eventgrid import (
    EventGridEventRepository,
    EventGridTopicRepository,
    InMemoryEventGridEventRepository,
    InMemoryEventGridTopicRepository,
    SqliteEventGridEventRepository,
    SqliteEventGridTopicRepository,
)
from cloudtwin.persistence.repositories.azure.functions import (
    AzureFunctionInvocationRepository,
    AzureFunctionRepository,
    InMemoryAzureFunctionInvocationRepository,
    InMemoryAzureFunctionRepository,
    SqliteAzureFunctionInvocationRepository,
    SqliteAzureFunctionRepository,
)
from cloudtwin.persistence.repositories.azure.keyvault import (
    InMemoryKeyVaultSecretRepository,
    KeyVaultSecretRepository,
    SqliteKeyVaultSecretRepository,
)
from cloudtwin.persistence.repositories.azure.queue import (
    AzureQueueMessageRepository,
    AzureStorageQueueRepository,
    InMemoryAzureQueueMessageRepository,
    InMemoryAzureStorageQueueRepository,
    SqliteAzureQueueMessageRepository,
    SqliteAzureStorageQueueRepository,
)
from cloudtwin.persistence.repositories.azure.servicebus import (
    AsbMessageRepository,
    AsbQueueRepository,
    AsbSubscriptionRepository,
    AsbTopicRepository,
    InMemoryAsbMessageRepository,
    InMemoryAsbQueueRepository,
    InMemoryAsbSubscriptionRepository,
    InMemoryAsbTopicRepository,
    SqliteAsbMessageRepository,
    SqliteAsbQueueRepository,
    SqliteAsbSubscriptionRepository,
    SqliteAsbTopicRepository,
)

__all__ = [
    # Blob
    "AzureContainerRepository",
    "AzureBlobRepository",
    "InMemoryAzureContainerRepository",
    "InMemoryAzureBlobRepository",
    "SqliteAzureContainerRepository",
    "SqliteAzureBlobRepository",
    # Service Bus
    "AsbQueueRepository",
    "AsbTopicRepository",
    "AsbSubscriptionRepository",
    "AsbMessageRepository",
    "InMemoryAsbQueueRepository",
    "InMemoryAsbTopicRepository",
    "InMemoryAsbSubscriptionRepository",
    "InMemoryAsbMessageRepository",
    "SqliteAsbQueueRepository",
    "SqliteAsbTopicRepository",
    "SqliteAsbSubscriptionRepository",
    "SqliteAsbMessageRepository",
    # Queue Storage
    "AzureStorageQueueRepository",
    "AzureQueueMessageRepository",
    "InMemoryAzureStorageQueueRepository",
    "InMemoryAzureQueueMessageRepository",
    "SqliteAzureStorageQueueRepository",
    "SqliteAzureQueueMessageRepository",
    # Key Vault
    "KeyVaultSecretRepository",
    "InMemoryKeyVaultSecretRepository",
    "SqliteKeyVaultSecretRepository",
    # Event Grid
    "EventGridTopicRepository",
    "EventGridEventRepository",
    "InMemoryEventGridTopicRepository",
    "InMemoryEventGridEventRepository",
    "SqliteEventGridTopicRepository",
    "SqliteEventGridEventRepository",
    # Functions
    "AzureFunctionRepository",
    "AzureFunctionInvocationRepository",
    "InMemoryAzureFunctionRepository",
    "InMemoryAzureFunctionInvocationRepository",
    "SqliteAzureFunctionRepository",
    "SqliteAzureFunctionInvocationRepository",
]
