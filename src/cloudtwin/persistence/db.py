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
from cloudtwin.persistence.repositories.aws.secretsmanager.sqlite import DDL as _SECRETSMANAGER_DDL
from cloudtwin.persistence.repositories.aws.dynamodb.sqlite import DDL as _DYNAMODB_DDL
from cloudtwin.persistence.repositories.aws.lambda_.sqlite import DDL as _LAMBDA_DDL
from cloudtwin.persistence.repositories.azure.blob.sqlite import DDL as _AZURE_BLOB_DDL
from cloudtwin.persistence.repositories.azure.servicebus.sqlite import DDL as _ASB_DDL
from cloudtwin.persistence.repositories.azure.queue.sqlite import DDL as _AZURE_QUEUE_DDL
from cloudtwin.persistence.repositories.azure.keyvault.sqlite import DDL as _KEYVAULT_DDL
from cloudtwin.persistence.repositories.azure.eventgrid.sqlite import DDL as _EVENTGRID_DDL
from cloudtwin.persistence.repositories.azure.functions.sqlite import DDL as _AZURE_FUNCTIONS_DDL
from cloudtwin.persistence.repositories.gcp.storage.sqlite import DDL as _GCS_DDL
from cloudtwin.persistence.repositories.gcp.pubsub.sqlite import DDL as _PUBSUB_DDL
from cloudtwin.persistence.repositories.gcp.firestore.sqlite import DDL as _FIRESTORE_DDL
from cloudtwin.persistence.repositories.gcp.cloudtasks.sqlite import DDL as _CLOUDTASKS_DDL
from cloudtwin.persistence.repositories.gcp.secretmanager.sqlite import DDL as _GCP_SECRETMANAGER_DDL
from cloudtwin.persistence.repositories.gcp.cloudfunctions.sqlite import DDL as _GCP_CLOUDFUNCTIONS_DDL
from cloudtwin.persistence.repositories.common.events.sqlite import DDL as _EVENTS_DDL

DDL = (
    # AWS
    _SES_DDL
    + _S3_DDL
    + _SNS_DDL
    + _SQS_DDL
    + _SECRETSMANAGER_DDL
    + _DYNAMODB_DDL
    + _LAMBDA_DDL
    # Azure
    + _AZURE_BLOB_DDL
    + _ASB_DDL
    + _AZURE_QUEUE_DDL
    + _KEYVAULT_DDL
    + _EVENTGRID_DDL
    + _AZURE_FUNCTIONS_DDL
    # GCP
    + _GCS_DDL
    + _PUBSUB_DDL
    + _FIRESTORE_DDL
    + _CLOUDTASKS_DDL
    + _GCP_SECRETMANAGER_DDL
    + _GCP_CLOUDFUNCTIONS_DDL
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
