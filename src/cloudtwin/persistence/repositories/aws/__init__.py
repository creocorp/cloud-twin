"""AWS repositories — public re-exports."""

from __future__ import annotations

from cloudtwin.persistence.repositories.aws.ses import (
    InMemorySesIdentityRepository,
    InMemorySesMessageRepository,
    SesIdentityRepository,
    SesMessageRepository,
    SqliteSesIdentityRepository,
    SqliteSesMessageRepository,
)
from cloudtwin.persistence.repositories.aws.s3 import (
    InMemoryS3BucketRepository,
    InMemoryS3ObjectRepository,
    S3BucketRepository,
    S3ObjectRepository,
    SqliteS3BucketRepository,
    SqliteS3ObjectRepository,
)
from cloudtwin.persistence.repositories.aws.sns import (
    InMemorySnsMessageRepository,
    InMemorySnsSubscriptionRepository,
    InMemorySnsTopicRepository,
    SnsMessageRepository,
    SnsSubscriptionRepository,
    SnsTopicRepository,
    SqliteSnsMessageRepository,
    SqliteSnsSubscriptionRepository,
    SqliteSnsTopicRepository,
)
from cloudtwin.persistence.repositories.aws.sqs import (
    InMemorySqsMessageRepository,
    InMemorySqsQueueRepository,
    SqsMessageRepository,
    SqsQueueRepository,
    SqliteSqsMessageRepository,
    SqliteSqsQueueRepository,
)

__all__ = [
    # SES
    "SesIdentityRepository", "SesMessageRepository",
    "SqliteSesIdentityRepository", "SqliteSesMessageRepository",
    "InMemorySesIdentityRepository", "InMemorySesMessageRepository",
    # S3
    "S3BucketRepository", "S3ObjectRepository",
    "SqliteS3BucketRepository", "SqliteS3ObjectRepository",
    "InMemoryS3BucketRepository", "InMemoryS3ObjectRepository",
    # SNS
    "SnsTopicRepository", "SnsSubscriptionRepository", "SnsMessageRepository",
    "SqliteSnsTopicRepository", "SqliteSnsSubscriptionRepository", "SqliteSnsMessageRepository",
    "InMemorySnsTopicRepository", "InMemorySnsSubscriptionRepository", "InMemorySnsMessageRepository",
    # SQS
    "SqsQueueRepository", "SqsMessageRepository",
    "SqliteSqsQueueRepository", "SqliteSqsMessageRepository",
    "InMemorySqsQueueRepository", "InMemorySqsMessageRepository",
]
