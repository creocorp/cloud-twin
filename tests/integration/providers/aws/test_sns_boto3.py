"""
SNS integration tests using boto3.

Uses a real uvicorn server (in-memory mode) via the session-scoped
server_url fixture from conftest.py.

SNS protocol: AWS Query (form-urlencoded at POST /)
No v2 REST variant exists for SNS.
"""

from __future__ import annotations

import pytest


# ---------------------------------------------------------------------------
# CreateTopic
# ---------------------------------------------------------------------------


def test_create_topic_returns_arn(sns):
    resp = sns.create_topic(Name="test-topic-1")
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200
    arn = resp["TopicArn"]
    assert "test-topic-1" in arn
    assert arn.startswith("arn:aws:sns:")


def test_create_topic_idempotent(sns):
    """Creating the same topic twice must return the same ARN."""
    arn1 = sns.create_topic(Name="idempotent-topic")["TopicArn"]
    arn2 = sns.create_topic(Name="idempotent-topic")["TopicArn"]
    assert arn1 == arn2


def test_create_multiple_topics(sns):
    for i in range(3):
        resp = sns.create_topic(Name=f"multi-topic-{i}")
        assert "TopicArn" in resp


# ---------------------------------------------------------------------------
# ListTopics
# ---------------------------------------------------------------------------


def test_list_topics_includes_created(sns):
    arn = sns.create_topic(Name="list-topic-a")["TopicArn"]
    resp = sns.list_topics()
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200
    arns = [t["TopicArn"] for t in resp["Topics"]]
    assert arn in arns


def test_list_topics_returns_all(sns):
    for i in range(3):
        sns.create_topic(Name=f"list-all-topic-{i}")
    resp = sns.list_topics()
    arns = [t["TopicArn"] for t in resp["Topics"]]
    assert len(arns) >= 3


# ---------------------------------------------------------------------------
# Subscribe
# ---------------------------------------------------------------------------


def test_subscribe_returns_subscription_arn(sns):
    topic_arn = sns.create_topic(Name="sub-topic-1")["TopicArn"]
    resp = sns.subscribe(
        TopicArn=topic_arn,
        Protocol="email",
        Endpoint="test@example.com",
    )
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200
    sub_arn = resp["SubscriptionArn"]
    assert topic_arn in sub_arn


def test_subscribe_idempotent(sns):
    """Same endpoint subscribed twice must return the same SubscriptionArn."""
    topic_arn = sns.create_topic(Name="sub-idempotent-topic")["TopicArn"]
    sub1 = sns.subscribe(TopicArn=topic_arn, Protocol="email", Endpoint="a@b.com")["SubscriptionArn"]
    sub2 = sns.subscribe(TopicArn=topic_arn, Protocol="email", Endpoint="a@b.com")["SubscriptionArn"]
    assert sub1 == sub2


def test_subscribe_different_endpoints_are_distinct(sns):
    topic_arn = sns.create_topic(Name="sub-multi-endpoint-topic")["TopicArn"]
    sub1 = sns.subscribe(TopicArn=topic_arn, Protocol="email", Endpoint="a@x.com")["SubscriptionArn"]
    sub2 = sns.subscribe(TopicArn=topic_arn, Protocol="email", Endpoint="b@x.com")["SubscriptionArn"]
    assert sub1 != sub2


def test_subscribe_nonexistent_topic_raises(sns):
    with pytest.raises(Exception, match="NotFound|not found|NoSuchEntity|NotFoundException|not exist"):
        sns.subscribe(
            TopicArn="arn:aws:sns:us-east-1:000000000000:ghost-topic",
            Protocol="email",
            Endpoint="x@example.com",
        )


# ---------------------------------------------------------------------------
# Publish
# ---------------------------------------------------------------------------


def test_publish_returns_message_id(sns):
    topic_arn = sns.create_topic(Name="publish-topic-1")["TopicArn"]
    resp = sns.publish(TopicArn=topic_arn, Message="Hello SNS!")
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200
    assert "MessageId" in resp
    assert len(resp["MessageId"]) > 0


def test_publish_with_subject(sns):
    topic_arn = sns.create_topic(Name="publish-subject-topic")["TopicArn"]
    resp = sns.publish(TopicArn=topic_arn, Message="Hello!", Subject="My Subject")
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200
    assert "MessageId" in resp


def test_publish_multiple_messages(sns):
    topic_arn = sns.create_topic(Name="publish-multi-topic")["TopicArn"]
    ids = set()
    for i in range(5):
        resp = sns.publish(TopicArn=topic_arn, Message=f"Message {i}")
        ids.add(resp["MessageId"])
    assert len(ids) == 5  # all unique


def test_publish_nonexistent_topic_raises(sns):
    with pytest.raises(Exception):
        sns.publish(
            TopicArn="arn:aws:sns:us-east-1:000000000000:ghost-topic",
            Message="will fail",
        )
