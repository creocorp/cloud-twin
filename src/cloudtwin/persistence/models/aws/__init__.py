"""AWS domain models — public re-exports."""

from __future__ import annotations

from cloudtwin.persistence.models.aws.s3 import S3Bucket, S3Object
from cloudtwin.persistence.models.aws.ses import SesIdentity, SesMessage
from cloudtwin.persistence.models.aws.sns import SnsMessage, SnsSubscription, SnsTopic
from cloudtwin.persistence.models.aws.sqs import SqsMessage, SqsQueue

__all__ = [
    # SES
    "SesIdentity",
    "SesMessage",
    # S3
    "S3Bucket",
    "S3Object",
    # SNS
    "SnsTopic",
    "SnsSubscription",
    "SnsMessage",
    # SQS
    "SqsQueue",
    "SqsMessage",
]
