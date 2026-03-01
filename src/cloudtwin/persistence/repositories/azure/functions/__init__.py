"""Azure Functions — repository package re-exports."""

from __future__ import annotations

from cloudtwin.persistence.repositories.azure.functions.inmemory import (
    InMemoryAzureFunctionInvocationRepository,
    InMemoryAzureFunctionRepository,
)
from cloudtwin.persistence.repositories.azure.functions.repository import (
    AzureFunctionInvocationRepository,
    AzureFunctionRepository,
)
from cloudtwin.persistence.repositories.azure.functions.sqlite import (
    SqliteAzureFunctionInvocationRepository,
    SqliteAzureFunctionRepository,
)

__all__ = [
    "AzureFunctionRepository",
    "AzureFunctionInvocationRepository",
    "SqliteAzureFunctionRepository",
    "SqliteAzureFunctionInvocationRepository",
    "InMemoryAzureFunctionRepository",
    "InMemoryAzureFunctionInvocationRepository",
]
