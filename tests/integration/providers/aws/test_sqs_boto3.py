"""
SQS integration tests using boto3.

Uses a real uvicorn server (in-memory mode) via the session-scoped
server_url fixture from conftest.py.

SQS protocol: AWS JSON (application/x-amz-json-1.0 at POST /)
No v2 REST variant exists for SQS.
"""

from __future__ import annotations

import hashlib

import pytest


def _md5(s: str) -> str:
    return hashlib.md5(s.encode()).hexdigest()  # noqa: S324


# ---------------------------------------------------------------------------
# CreateQueue / GetQueueUrl
# ---------------------------------------------------------------------------


def test_create_queue_returns_url(sqs):
    resp = sqs.create_queue(QueueName="test-queue-1")
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200
    url = resp["QueueUrl"]
    assert "test-queue-1" in url


def test_create_queue_idempotent(sqs):
    """Creating the same queue twice must return the same URL."""
    url1 = sqs.create_queue(QueueName="idem-queue")["QueueUrl"]
    url2 = sqs.create_queue(QueueName="idem-queue")["QueueUrl"]
    assert url1 == url2


def test_get_queue_url(sqs):
    url_created = sqs.create_queue(QueueName="get-url-queue")["QueueUrl"]
    resp = sqs.get_queue_url(QueueName="get-url-queue")
    assert resp["QueueUrl"] == url_created


def test_get_queue_url_nonexistent_raises(sqs):
    with pytest.raises(Exception, match="NonExistentQueue|not exist|not found"):
        sqs.get_queue_url(QueueName="ghost-queue-xyz-12345")


# ---------------------------------------------------------------------------
# ListQueues
# ---------------------------------------------------------------------------


def test_list_queues_includes_created(sqs):
    url = sqs.create_queue(QueueName="list-queue-a")["QueueUrl"]
    resp = sqs.list_queues()
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200
    assert url in resp.get("QueueUrls", [])


def test_list_queues_with_prefix(sqs):
    sqs.create_queue(QueueName="prefix-queue-1")
    sqs.create_queue(QueueName="prefix-queue-2")
    sqs.create_queue(QueueName="other-queue-1")
    resp = sqs.list_queues(QueueNamePrefix="prefix-queue")
    urls = resp.get("QueueUrls", [])
    assert all("prefix-queue" in u for u in urls)
    assert all("other-queue" not in u for u in urls)


# ---------------------------------------------------------------------------
# SendMessage
# ---------------------------------------------------------------------------


def test_send_message_returns_id_and_md5(sqs):
    url = sqs.create_queue(QueueName="send-queue-1")["QueueUrl"]
    body = "Hello SQS!"
    resp = sqs.send_message(QueueUrl=url, MessageBody=body)
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200
    assert "MessageId" in resp
    assert resp["MD5OfMessageBody"] == _md5(body)


def test_send_multiple_messages_unique_ids(sqs):
    url = sqs.create_queue(QueueName="send-multi-queue")["QueueUrl"]
    ids = {sqs.send_message(QueueUrl=url, MessageBody=f"msg {i}")["MessageId"] for i in range(5)}
    assert len(ids) == 5


def test_send_message_nonexistent_queue_raises(sqs, server_url):
    fake_url = f"{server_url}/000000000000/totally-fake-queue"
    with pytest.raises(Exception):
        sqs.send_message(QueueUrl=fake_url, MessageBody="boom")


# ---------------------------------------------------------------------------
# ReceiveMessage
# ---------------------------------------------------------------------------


def test_receive_message_returns_sent_body(sqs):
    url = sqs.create_queue(QueueName="recv-queue-1")["QueueUrl"]
    body = "receive-me"
    sqs.send_message(QueueUrl=url, MessageBody=body)
    resp = sqs.receive_message(QueueUrl=url)
    assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200
    messages = resp.get("Messages", [])
    assert len(messages) == 1
    assert messages[0]["Body"] == body
    assert messages[0]["MD5OfBody"] == _md5(body)
    assert "MessageId" in messages[0]
    assert "ReceiptHandle" in messages[0]


def test_receive_message_empty_queue_returns_none(sqs):
    url = sqs.create_queue(QueueName="empty-queue-recv")["QueueUrl"]
    resp = sqs.receive_message(QueueUrl=url)
    assert resp.get("Messages", []) == []


def test_receive_message_max_number(sqs):
    url = sqs.create_queue(QueueName="max-recv-queue")["QueueUrl"]
    for i in range(5):
        sqs.send_message(QueueUrl=url, MessageBody=f"msg {i}")
    resp = sqs.receive_message(QueueUrl=url, MaxNumberOfMessages=3)
    assert len(resp.get("Messages", [])) == 3


def test_received_message_not_visible_again(sqs):
    """After receiving, a message should not be visible without deletion."""
    url = sqs.create_queue(QueueName="invisible-queue")["QueueUrl"]
    sqs.send_message(QueueUrl=url, MessageBody="once-only")
    # First receive – should get the message
    first = sqs.receive_message(QueueUrl=url).get("Messages", [])
    assert len(first) == 1
    # Second receive (before deletion) – message should be invisible
    second = sqs.receive_message(QueueUrl=url).get("Messages", [])
    assert len(second) == 0


# ---------------------------------------------------------------------------
# DeleteMessage
# ---------------------------------------------------------------------------


def test_delete_message_removes_it(sqs):
    url = sqs.create_queue(QueueName="delete-msg-queue")["QueueUrl"]
    sqs.send_message(QueueUrl=url, MessageBody="bye")
    receipt = sqs.receive_message(QueueUrl=url)["Messages"][0]["ReceiptHandle"]
    del_resp = sqs.delete_message(QueueUrl=url, ReceiptHandle=receipt)
    assert del_resp["ResponseMetadata"]["HTTPStatusCode"] == 200


def test_delete_message_idempotent(sqs):
    """Deleting the same receipt handle twice should not raise."""
    url = sqs.create_queue(QueueName="delete-idem-queue")["QueueUrl"]
    sqs.send_message(QueueUrl=url, MessageBody="del-me")
    receipt = sqs.receive_message(QueueUrl=url)["Messages"][0]["ReceiptHandle"]
    sqs.delete_message(QueueUrl=url, ReceiptHandle=receipt)
    # Second delete should not raise
    sqs.delete_message(QueueUrl=url, ReceiptHandle=receipt)


# ---------------------------------------------------------------------------
# Full send → receive → delete lifecycle
# ---------------------------------------------------------------------------


def test_full_lifecycle(sqs):
    url = sqs.create_queue(QueueName="lifecycle-queue")["QueueUrl"]

    # Send
    body = "full-lifecycle-message"
    send_resp = sqs.send_message(QueueUrl=url, MessageBody=body)
    message_id = send_resp["MessageId"]

    # Receive
    recv_resp = sqs.receive_message(QueueUrl=url, MaxNumberOfMessages=1)
    messages = recv_resp["Messages"]
    assert len(messages) == 1
    msg = messages[0]
    assert msg["Body"] == body
    assert msg["MessageId"] == message_id

    # Delete
    sqs.delete_message(QueueUrl=url, ReceiptHandle=msg["ReceiptHandle"])

    # Confirm gone – queue should be empty now
    final = sqs.receive_message(QueueUrl=url).get("Messages", [])
    assert final == []
