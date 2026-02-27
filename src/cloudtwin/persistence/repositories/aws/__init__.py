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
from cloudtwin.persistence.repositories.aws.secretsmanager import (
    InMemorySecretRepository,
    InMemorySecretVersionRepository,
    SecretRepository,
    SecretVersionRepository,
    SqliteSecretRepository,
    SqliteSecretVersionRepository,
)
from cloudtwin.persistence.repositories.aws.dynamodb import (
    DynamoItemRepository,
    DynamoTableRepository,
    InMemoryDynamoItemRepository,
    InMemoryDynamoTableRepository,
    SqliteDynamoItemRepository,
    SqliteDynamoTableRepository,
)
from cloudtwin.persistence.repositories.aws.lambda_ import (
    InMemoryLambdaFunctionRepository,
    InMemoryLambdaInvocationRepository,
    LambdaFunctionRepository,
    LambdaInvocationRepository,
    SqliteLambdaFunctionRepository,
    SqliteLambdaInvocationRepository,
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
    # Secrets Manager
    "SecretRepository", "SecretVersionRepository",
    "SqliteSecretRepository", "SqliteSecretVersionRepository",
    "InMemorySecretRepository", "InMemorySecretVersionRepository",
    # DynamoDB
    "DynamoTableRepository", "DynamoItemRepository",
    "SqliteDynamoTableRepository", "SqliteDynamoItemRepository",
    "InMemoryDynamoTableRepository", "InMemoryDynamoItemRepository",
    # Lambda
    "LambdaFunctionRepository", "LambdaInvocationRepository",
    "SqliteLambdaFunctionRepository", "SqliteLambdaInvocationRepository",
    "InMemoryLambdaFunctionRepository", "InMemoryLambdaInvocationRepository",
]
