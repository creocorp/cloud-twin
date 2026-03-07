"""SES repositories — public re-exports."""

from __future__ import annotations

from cloudtwin.persistence.repositories.aws.ses.repository import (
    SesIdentityRepository,
    SesMessageRepository,
)
from cloudtwin.persistence.repositories.aws.ses.sqlite import (
    SqliteSesIdentityRepository,
    SqliteSesMessageRepository,
)

__all__ = [
    "SesIdentityRepository",
    "SesMessageRepository",
    "SqliteSesIdentityRepository",
    "SqliteSesMessageRepository",
]
