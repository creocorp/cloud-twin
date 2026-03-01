"""SES repositories — public re-exports."""

from __future__ import annotations

from cloudtwin.persistence.repositories.aws.ses.inmemory import (
    InMemorySesIdentityRepository,
    InMemorySesMessageRepository,
)
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
    "InMemorySesIdentityRepository",
    "InMemorySesMessageRepository",
    "SqliteSesIdentityRepository",
    "SqliteSesMessageRepository",
]
