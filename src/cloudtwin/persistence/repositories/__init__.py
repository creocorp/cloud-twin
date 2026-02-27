"""
Persistence repositories — flat re-export of all abstract interfaces and implementations,
plus the make_repositories() factory.
"""

from __future__ import annotations

from cloudtwin.persistence.repositories.aws import (
    InMemoryS3BucketRepository,
    InMemoryS3ObjectRepository,
    InMemorySesIdentityRepository,
    InMemorySesMessageRepository,
    InMemorySnsMessageRepository,
    InMemorySnsSubscriptionRepository,
    InMemorySnsTopicRepository,
    InMemorySqsMessageRepository,
    InMemorySqsQueueRepository,
    S3BucketRepository,
    S3ObjectRepository,
    SesIdentityRepository,
    SesMessageRepository,
    SnsMessageRepository,
    SnsSubscriptionRepository,
    SnsTopicRepository,
    SqliteS3BucketRepository,
    SqliteS3ObjectRepository,
    SqliteSesIdentityRepository,
    SqliteSesMessageRepository,
    SqliteSnsMessageRepository,
    SqliteSnsSubscriptionRepository,
    SqliteSnsTopicRepository,
    SqliteSqsMessageRepository,
    SqliteSqsQueueRepository,
    SqsMessageRepository,
    SqsQueueRepository,
)
from cloudtwin.persistence.repositories.azure import (
    AsbMessageRepository,
    AsbQueueRepository,
    AsbSubscriptionRepository,
    AsbTopicRepository,
    AzureBlobRepository,
    AzureContainerRepository,
    InMemoryAsbMessageRepository,
    InMemoryAsbQueueRepository,
    InMemoryAsbSubscriptionRepository,
    InMemoryAsbTopicRepository,
    InMemoryAzureBlobRepository,
    InMemoryAzureContainerRepository,
    SqliteAsbMessageRepository,
    SqliteAsbQueueRepository,
    SqliteAsbSubscriptionRepository,
    SqliteAsbTopicRepository,
    SqliteAzureBlobRepository,
    SqliteAzureContainerRepository,
)
from cloudtwin.persistence.repositories.common import (
    EventRepository,
    InMemoryEventRepository,
    SqliteEventRepository,
)
from cloudtwin.persistence.repositories.gcp import (
    GcsBucketRepository,
    GcsObjectRepository,
    InMemoryGcsBucketRepository,
    InMemoryGcsObjectRepository,
    InMemoryPubsubAckableRepository,
    InMemoryPubsubMessageRepository,
    InMemoryPubsubSubscriptionRepository,
    InMemoryPubsubTopicRepository,
    PubsubAckableRepository,
    PubsubMessageRepository,
    PubsubSubscriptionRepository,
    PubsubTopicRepository,
    SqliteGcsBucketRepository,
    SqliteGcsObjectRepository,
    SqlitePubsubAckableRepository,
    SqlitePubsubMessageRepository,
    SqlitePubsubSubscriptionRepository,
    SqlitePubsubTopicRepository,
)

__all__ = [
    # AWS
    "SesIdentityRepository", "SesMessageRepository",
    "S3BucketRepository", "S3ObjectRepository",
    "SnsTopicRepository", "SnsSubscriptionRepository", "SnsMessageRepository",
    "SqsQueueRepository", "SqsMessageRepository",
    "SqliteSesIdentityRepository", "SqliteSesMessageRepository",
    "SqliteS3BucketRepository", "SqliteS3ObjectRepository",
    "SqliteSnsTopicRepository", "SqliteSnsSubscriptionRepository", "SqliteSnsMessageRepository",
    "SqliteSqsQueueRepository", "SqliteSqsMessageRepository",
    "InMemorySesIdentityRepository", "InMemorySesMessageRepository",
    "InMemoryS3BucketRepository", "InMemoryS3ObjectRepository",
    "InMemorySnsTopicRepository", "InMemorySnsSubscriptionRepository", "InMemorySnsMessageRepository",
    "InMemorySqsQueueRepository", "InMemorySqsMessageRepository",
    # Azure
    "AzureContainerRepository", "AzureBlobRepository",
    "AsbQueueRepository", "AsbTopicRepository", "AsbSubscriptionRepository", "AsbMessageRepository",
    "SqliteAzureContainerRepository", "SqliteAzureBlobRepository",
    "SqliteAsbQueueRepository", "SqliteAsbTopicRepository",
    "SqliteAsbSubscriptionRepository", "SqliteAsbMessageRepository",
    "InMemoryAzureContainerRepository", "InMemoryAzureBlobRepository",
    "InMemoryAsbQueueRepository", "InMemoryAsbTopicRepository",
    "InMemoryAsbSubscriptionRepository", "InMemoryAsbMessageRepository",
    # GCP
    "GcsBucketRepository", "GcsObjectRepository",
    "PubsubTopicRepository", "PubsubSubscriptionRepository",
    "PubsubMessageRepository", "PubsubAckableRepository",
    "SqliteGcsBucketRepository", "SqliteGcsObjectRepository",
    "SqlitePubsubTopicRepository", "SqlitePubsubSubscriptionRepository",
    "SqlitePubsubMessageRepository", "SqlitePubsubAckableRepository",
    "InMemoryGcsBucketRepository", "InMemoryGcsObjectRepository",
    "InMemoryPubsubTopicRepository", "InMemoryPubsubSubscriptionRepository",
    "InMemoryPubsubMessageRepository", "InMemoryPubsubAckableRepository",
    # Common
    "EventRepository", "SqliteEventRepository", "InMemoryEventRepository",
    # Factory
    "make_repositories",
]


def make_repositories(db, mode: str = "sqlite") -> dict:
    """Return a dict of all repository instances for the selected storage mode."""
    if mode == "memory":
        return {
            # AWS
            "ses_identity": InMemorySesIdentityRepository(),
            "ses_message": InMemorySesMessageRepository(),
            "s3_bucket": InMemoryS3BucketRepository(),
            "s3_object": InMemoryS3ObjectRepository(),
            "sns_topic": InMemorySnsTopicRepository(),
            "sns_subscription": InMemorySnsSubscriptionRepository(),
            "sns_message": InMemorySnsMessageRepository(),
            "sqs_queue": InMemorySqsQueueRepository(),
            "sqs_message": InMemorySqsMessageRepository(),
            # Azure
            "azure_container": InMemoryAzureContainerRepository(),
            "azure_blob": InMemoryAzureBlobRepository(),
            "asb_queue": InMemoryAsbQueueRepository(),
            "asb_topic": InMemoryAsbTopicRepository(),
            "asb_subscription": InMemoryAsbSubscriptionRepository(),
            "asb_message": InMemoryAsbMessageRepository(),
            # GCP
            "gcs_bucket": InMemoryGcsBucketRepository(),
            "gcs_object": InMemoryGcsObjectRepository(),
            "pubsub_topic": InMemoryPubsubTopicRepository(),
            "pubsub_subscription": InMemoryPubsubSubscriptionRepository(),
            "pubsub_message": InMemoryPubsubMessageRepository(),
            "pubsub_ackable": InMemoryPubsubAckableRepository(),
            # Common
            "event": InMemoryEventRepository(),
        }
    # SQLite mode
    return {
        # AWS
        "ses_identity": SqliteSesIdentityRepository(db),
        "ses_message": SqliteSesMessageRepository(db),
        "s3_bucket": SqliteS3BucketRepository(db),
        "s3_object": SqliteS3ObjectRepository(db),
        "sns_topic": SqliteSnsTopicRepository(db),
        "sns_subscription": SqliteSnsSubscriptionRepository(db),
        "sns_message": SqliteSnsMessageRepository(db),
        "sqs_queue": SqliteSqsQueueRepository(db),
        "sqs_message": SqliteSqsMessageRepository(db),
        # Azure
        "azure_container": SqliteAzureContainerRepository(db),
        "azure_blob": SqliteAzureBlobRepository(db),
        "asb_queue": SqliteAsbQueueRepository(db),
        "asb_topic": SqliteAsbTopicRepository(db),
        "asb_subscription": SqliteAsbSubscriptionRepository(db),
        "asb_message": SqliteAsbMessageRepository(db),
        # GCP
        "gcs_bucket": SqliteGcsBucketRepository(db),
        "gcs_object": SqliteGcsObjectRepository(db),
        "pubsub_topic": SqlitePubsubTopicRepository(db),
        "pubsub_subscription": SqlitePubsubSubscriptionRepository(db),
        "pubsub_message": SqlitePubsubMessageRepository(db),
        "pubsub_ackable": SqlitePubsubAckableRepository(db),
        # Common
        "event": SqliteEventRepository(db),
    }
