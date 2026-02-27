"""Azure Key Vault — repository package re-exports."""

from __future__ import annotations

from cloudtwin.persistence.repositories.azure.keyvault.inmemory import InMemoryKeyVaultSecretRepository
from cloudtwin.persistence.repositories.azure.keyvault.repository import KeyVaultSecretRepository
from cloudtwin.persistence.repositories.azure.keyvault.sqlite import SqliteKeyVaultSecretRepository

__all__ = [
    "KeyVaultSecretRepository",
    "SqliteKeyVaultSecretRepository",
    "InMemoryKeyVaultSecretRepository",
]
