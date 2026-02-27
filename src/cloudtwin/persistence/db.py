"""
SQLite database manager.
Handles connection lifecycle and schema creation.

DDL is collected from each service's sqlite module so that schema definitions
live alongside their repository implementations.
"""

from __future__ import annotations

import aiosqlite

from cloudtwin.config import StorageConfig
from cloudtwin.persistence.repositories.aws.ses.sqlite import DDL as _SES_DDL
from cloudtwin.persistence.repositories.aws.s3.sqlite import DDL as _S3_DDL
from cloudtwin.persistence.repositories.aws.sns.sqlite import DDL as _SNS_DDL
from cloudtwin.persistence.repositories.aws.sqs.sqlite import DDL as _SQS_DDL
from cloudtwin.persistence.repositories.azure.blob.sqlite import DDL as _AZURE_BLOB_DDL
from cloudtwin.persistence.repositories.azure.servicebus.sqlite import DDL as _ASB_DDL
from cloudtwin.persistence.repositories.gcp.storage.sqlite import DDL as _GCS_DDL
from cloudtwin.persistence.repositories.gcp.pubsub.sqlite import DDL as _PUBSUB_DDL
from cloudtwin.persistence.repositories.common.events.sqlite import DDL as _EVENTS_DDL

DDL = (
    # AWS
    _SES_DDL
    + _S3_DDL
    + _SNS_DDL
    + _SQS_DDL
    # Azure
    + _AZURE_BLOB_DDL
    + _ASB_DDL
    # GCP
    + _GCS_DDL
    + _PUBSUB_DDL
    # Common
    + _EVENTS_DDL
)


class Database:
    def __init__(self, config: StorageConfig):
        self._config = config
        self._conn: aiosqlite.Connection | None = None

    @property
    def conn(self) -> aiosqlite.Connection:
        if self._conn is None:
            raise RuntimeError("Database not connected")
        return self._conn

    async def connect(self) -> None:
        if self._config.mode == "memory":
            self._conn = await aiosqlite.connect(":memory:")
        else:
            import os
            os.makedirs(os.path.dirname(self._config.path), exist_ok=True)
            self._conn = await aiosqlite.connect(self._config.path)

        self._conn.row_factory = aiosqlite.Row
        await self._conn.executescript(DDL)
        await self._conn.commit()

    async def disconnect(self) -> None:
        if self._conn:
            await self._conn.close()
            self._conn = None
