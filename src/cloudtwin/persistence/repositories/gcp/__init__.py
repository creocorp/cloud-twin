"""GCP repositories — public re-exports."""

from __future__ import annotations

from cloudtwin.persistence.repositories.gcp.storage import (
    GcsBucketRepository,
    GcsObjectRepository,
    InMemoryGcsBucketRepository,
    InMemoryGcsObjectRepository,
    SqliteGcsBucketRepository,
    SqliteGcsObjectRepository,
)
from cloudtwin.persistence.repositories.gcp.pubsub import (
    PubsubTopicRepository,
    PubsubSubscriptionRepository,
    PubsubMessageRepository,
    PubsubAckableRepository,
    InMemoryPubsubTopicRepository,
    InMemoryPubsubSubscriptionRepository,
    InMemoryPubsubMessageRepository,
    InMemoryPubsubAckableRepository,
    SqlitePubsubTopicRepository,
    SqlitePubsubSubscriptionRepository,
    SqlitePubsubMessageRepository,
    SqlitePubsubAckableRepository,
)
from cloudtwin.persistence.repositories.gcp.firestore import (
    FirestoreDocumentRepository,
    InMemoryFirestoreDocumentRepository,
    SqliteFirestoreDocumentRepository,
)
from cloudtwin.persistence.repositories.gcp.cloudtasks import (
    CloudTasksQueueRepository,
    CloudTaskRepository,
    InMemoryCloudTasksQueueRepository,
    InMemoryCloudTaskRepository,
    SqliteCloudTasksQueueRepository,
    SqliteCloudTaskRepository,
)
from cloudtwin.persistence.repositories.gcp.secretmanager import (
    GcpSecretRepository,
    GcpSecretVersionRepository,
    InMemoryGcpSecretRepository,
    InMemoryGcpSecretVersionRepository,
    SqliteGcpSecretRepository,
    SqliteGcpSecretVersionRepository,
)
from cloudtwin.persistence.repositories.gcp.cloudfunctions import (
    CloudFunctionRepository,
    CloudFunctionInvocationRepository,
    InMemoryCloudFunctionRepository,
    InMemoryCloudFunctionInvocationRepository,
    SqliteCloudFunctionRepository,
    SqliteCloudFunctionInvocationRepository,
)

__all__ = [
    # Storage
    "GcsBucketRepository", "GcsObjectRepository",
    "InMemoryGcsBucketRepository", "InMemoryGcsObjectRepository",
    "SqliteGcsBucketRepository", "SqliteGcsObjectRepository",
    # Pub/Sub
    "PubsubTopicRepository", "PubsubSubscriptionRepository",
    "PubsubMessageRepository", "PubsubAckableRepository",
    "InMemoryPubsubTopicRepository", "InMemoryPubsubSubscriptionRepository",
    "InMemoryPubsubMessageRepository", "InMemoryPubsubAckableRepository",
    "SqlitePubsubTopicRepository", "SqlitePubsubSubscriptionRepository",
    "SqlitePubsubMessageRepository", "SqlitePubsubAckableRepository",
    # Firestore
    "FirestoreCollectionRepository", "FirestoreDocumentRepository",
    "InMemoryFirestoreCollectionRepository", "InMemoryFirestoreDocumentRepository",
    "SqliteFirestoreCollectionRepository", "SqliteFirestoreDocumentRepository",
    # Cloud Tasks
    "CloudTasksQueueRepository", "CloudTaskRepository",
    "InMemoryCloudTasksQueueRepository", "InMemoryCloudTaskRepository",
    "SqliteCloudTasksQueueRepository", "SqliteCloudTaskRepository",
    # Secret Manager
    "GcpSecretRepository", "GcpSecretVersionRepository",
    "InMemoryGcpSecretRepository", "InMemoryGcpSecretVersionRepository",
    "SqliteGcpSecretRepository", "SqliteGcpSecretVersionRepository",
    # Cloud Functions
    "CloudFunctionRepository", "CloudFunctionInvocationRepository",
    "InMemoryCloudFunctionRepository", "InMemoryCloudFunctionInvocationRepository",
    "SqliteCloudFunctionRepository", "SqliteCloudFunctionInvocationRepository",
]
