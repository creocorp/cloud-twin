"""AWS repositories — public re-exports."""

from __future__ import annotations

from cloudtwin.persistence.repositories.aws.dynamodb import (
    DynamoItemRepository,
    DynamoTableRepository,
    SqliteDynamoItemRepository,
    SqliteDynamoTableRepository,
)
from cloudtwin.persistence.repositories.aws.lambda_ import (
    LambdaFunctionRepository,
    LambdaInvocationRepository,
    SqliteLambdaFunctionRepository,
    SqliteLambdaInvocationRepository,
)
from cloudtwin.persistence.repositories.aws.s3 import (
    S3BucketRepository,
    S3ObjectRepository,
    SqliteS3BucketRepository,
    SqliteS3ObjectRepository,
)
from cloudtwin.persistence.repositories.aws.secretsmanager import (
    SecretRepository,
    SecretVersionRepository,
    SqliteSecretRepository,
    SqliteSecretVersionRepository,
)
from cloudtwin.persistence.repositories.aws.ses import (
    SesIdentityRepository,
    SesMessageRepository,
    SqliteSesIdentityRepository,
    SqliteSesMessageRepository,
)
from cloudtwin.persistence.repositories.aws.sns import (
    SnsMessageRepository,
    SnsSubscriptionRepository,
    SnsTopicRepository,
    SqliteSnsMessageRepository,
    SqliteSnsSubscriptionRepository,
    SqliteSnsTopicRepository,
)
from cloudtwin.persistence.repositories.aws.sqs import (
    SqliteSqsMessageRepository,
    SqliteSqsQueueRepository,
    SqsMessageRepository,
    SqsQueueRepository,
)

__all__ = [
    # SES
    "SesIdentityRepository",
    "SesMessageRepository",
    "SqliteSesIdentityRepository",
    "SqliteSesMessageRepository",
    # S3
    "S3BucketRepository",
    "S3ObjectRepository",
    "SqliteS3BucketRepository",
    "SqliteS3ObjectRepository",
    # SNS
    "SnsTopicRepository",
    "SnsSubscriptionRepository",
    "SnsMessageRepository",
    "SqliteSnsTopicRepository",
    "SqliteSnsSubscriptionRepository",
    "SqliteSnsMessageRepository",
    # SQS
    "SqsQueueRepository",
    "SqsMessageRepository",
    "SqliteSqsQueueRepository",
    "SqliteSqsMessageRepository",
    # Secrets Manager
    "SecretRepository",
    "SecretVersionRepository",
    "SqliteSecretRepository",
    "SqliteSecretVersionRepository",
    # DynamoDB
    "DynamoTableRepository",
    "DynamoItemRepository",
    "SqliteDynamoTableRepository",
    "SqliteDynamoItemRepository",
    # Lambda
    "LambdaFunctionRepository",
    "LambdaInvocationRepository",
    "SqliteLambdaFunctionRepository",
    "SqliteLambdaInvocationRepository",
]
