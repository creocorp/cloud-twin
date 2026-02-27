"""
Persistence models package.

Re-exports all domain model dataclasses from provider-specific modules.
Import from here for a flat namespace: ``from cloudtwin.persistence.models import SesIdentity``
"""

from __future__ import annotations

from cloudtwin.persistence.models.aws import (
    S3Bucket,
    S3Object,
    SesIdentity,
    SesMessage,
    SnsMessage,
    SnsSubscription,
    SnsTopic,
    SqsMessage,
    SqsQueue,
)
from cloudtwin.persistence.models.azure import (
    AsbMessage,
    AsbQueue,
    AsbSubscription,
    AsbTopic,
    AzureBlob,
    AzureContainer,
)
from cloudtwin.persistence.models.common import Event
from cloudtwin.persistence.models.gcp import (
    GcsBucket,
    GcsObject,
    PubsubAckable,
    PubsubMessage,
    PubsubSubscription,
    PubsubTopic,
)

__all__ = [
    # AWS / SES
    "SesIdentity",
    "SesMessage",
    # AWS / S3
    "S3Bucket",
    "S3Object",
    # AWS / SNS
    "SnsTopic",
    "SnsSubscription",
    "SnsMessage",
    # AWS / SQS
    "SqsQueue",
    "SqsMessage",
    # Azure Blob
    "AzureContainer",
    "AzureBlob",
    # Azure Service Bus
    "AsbQueue",
    "AsbTopic",
    "AsbSubscription",
    "AsbMessage",
    # GCP Storage
    "GcsBucket",
    "GcsObject",
    # GCP Pub/Sub
    "PubsubTopic",
    "PubsubSubscription",
    "PubsubMessage",
    "PubsubAckable",
    # Common
    "Event",
]
