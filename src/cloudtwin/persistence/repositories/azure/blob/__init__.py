"""Azure Blob repositories — public re-exports."""

from __future__ import annotations

from cloudtwin.persistence.repositories.azure.blob.repository import (
    AzureContainerRepository,
    AzureBlobRepository,
)
from cloudtwin.persistence.repositories.azure.blob.inmemory import (
    InMemoryAzureContainerRepository,
    InMemoryAzureBlobRepository,
)
from cloudtwin.persistence.repositories.azure.blob.sqlite import (
    SqliteAzureContainerRepository,
    SqliteAzureBlobRepository,
)

__all__ = [
    "AzureContainerRepository",
    "AzureBlobRepository",
    "InMemoryAzureContainerRepository",
    "InMemoryAzureBlobRepository",
    "SqliteAzureContainerRepository",
    "SqliteAzureBlobRepository",
]
