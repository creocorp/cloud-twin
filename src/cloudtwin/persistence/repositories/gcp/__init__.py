"""GCP repositories — public re-exports."""

from __future__ import annotations

from cloudtwin.persistence.repositories.gcp.cloudfunctions import (
    CloudFunctionInvocationRepository,
    CloudFunctionRepository,
    SqliteCloudFunctionInvocationRepository,
    SqliteCloudFunctionRepository,
)
from cloudtwin.persistence.repositories.gcp.cloudtasks import (
    CloudTaskRepository,
    CloudTasksQueueRepository,
    SqliteCloudTaskRepository,
    SqliteCloudTasksQueueRepository,
)
from cloudtwin.persistence.repositories.gcp.firestore import (
    FirestoreDocumentRepository,
    SqliteFirestoreDocumentRepository,
)
from cloudtwin.persistence.repositories.gcp.pubsub import (
    PubsubAckableRepository,
    PubsubMessageRepository,
    PubsubSubscriptionRepository,
    PubsubTopicRepository,
    SqlitePubsubAckableRepository,
    SqlitePubsubMessageRepository,
    SqlitePubsubSubscriptionRepository,
    SqlitePubsubTopicRepository,
)
from cloudtwin.persistence.repositories.gcp.secretmanager import (
    GcpSecretRepository,
    GcpSecretVersionRepository,
    SqliteGcpSecretRepository,
    SqliteGcpSecretVersionRepository,
)
from cloudtwin.persistence.repositories.gcp.storage import (
    GcsBucketRepository,
    GcsObjectRepository,
    SqliteGcsBucketRepository,
    SqliteGcsObjectRepository,
)

__all__ = [
    # Storage
    "GcsBucketRepository",
    "GcsObjectRepository",
    "SqliteGcsBucketRepository",
    "SqliteGcsObjectRepository",
    # Pub/Sub
    "PubsubTopicRepository",
    "PubsubSubscriptionRepository",
    "PubsubMessageRepository",
    "PubsubAckableRepository",
    "SqlitePubsubTopicRepository",
    "SqlitePubsubSubscriptionRepository",
    "SqlitePubsubMessageRepository",
    "SqlitePubsubAckableRepository",
    # Firestore
    "FirestoreDocumentRepository",
    "SqliteFirestoreDocumentRepository",
    # Cloud Tasks
    "CloudTasksQueueRepository",
    "CloudTaskRepository",
    "SqliteCloudTasksQueueRepository",
    "SqliteCloudTaskRepository",
    # Secret Manager
    "GcpSecretRepository",
    "GcpSecretVersionRepository",
    "SqliteGcpSecretRepository",
    "SqliteGcpSecretVersionRepository",
    # Cloud Functions
    "CloudFunctionRepository",
    "CloudFunctionInvocationRepository",
    "SqliteCloudFunctionRepository",
    "SqliteCloudFunctionInvocationRepository",
]
