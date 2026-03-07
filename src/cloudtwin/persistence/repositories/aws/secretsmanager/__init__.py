"""AWS Secrets Manager — repository package re-exports."""

from __future__ import annotations

from cloudtwin.persistence.repositories.aws.secretsmanager.repository import (
    SecretRepository,
    SecretVersionRepository,
)
from cloudtwin.persistence.repositories.aws.secretsmanager.sqlite import (
    SqliteSecretRepository,
    SqliteSecretVersionRepository,
)

__all__ = [
    "SecretRepository",
    "SecretVersionRepository",
    "SqliteSecretRepository",
    "SqliteSecretVersionRepository",
]
