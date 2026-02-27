"""GCP Pub/Sub integration tests using the google-cloud-pubsub SDK (REST transport)."""

from __future__ import annotations

import base64
import uuid

_PROJECT = "cloudtwin-local"


def _topic_path(name: str) -> str:
    return f"projects/{_PROJECT}/topics/{name}"


def _sub_path(name: str) -> str:
    return f"projects/{_PROJECT}/subscriptions/{name}"


def _name() -> str:
    return f"ct-{uuid.uuid4().hex[:8]}"


class TestTopics:
    def test_create_topic(self, pubsub_publisher):
        name = _name()
        topic = pubsub_publisher.create_topic(name=_topic_path(name))
        assert topic.name == _topic_path(name)

    def test_list_topics(self, pubsub_publisher):
        name = _name()
        pubsub_publisher.create_topic(name=_topic_path(name))
        topics = list(pubsub_publisher.list_topics(request={"project": f"projects/{_PROJECT}"}))
        topic_names = [t.name for t in topics]
        assert _topic_path(name) in topic_names

    def test_delete_topic(self, pubsub_publisher):
        name = _name()
        pubsub_publisher.create_topic(name=_topic_path(name))
        pubsub_publisher.delete_topic(topic=_topic_path(name))
        # After deletion, it should not appear in list
        topics = list(pubsub_publisher.list_topics(request={"project": f"projects/{_PROJECT}"}))
        assert _topic_path(name) not in [t.name for t in topics]


class TestSubscriptions:
    def test_create_subscription(self, pubsub_publisher, pubsub_subscriber):
        topic_name = _name()
        sub_name = _name()
        pubsub_publisher.create_topic(name=_topic_path(topic_name))
        sub = pubsub_subscriber.create_subscription(
            request={"name": _sub_path(sub_name), "topic": _topic_path(topic_name)}
        )
        assert sub.name == _sub_path(sub_name)
        assert sub.topic == _topic_path(topic_name)

    def test_list_subscriptions(self, pubsub_publisher, pubsub_subscriber):
        topic_name = _name()
        sub_name = _name()
        pubsub_publisher.create_topic(name=_topic_path(topic_name))
        pubsub_subscriber.create_subscription(
            request={"name": _sub_path(sub_name), "topic": _topic_path(topic_name)}
        )
        subs = list(pubsub_subscriber.list_subscriptions(
            request={"project": f"projects/{_PROJECT}"}
        ))
        sub_names = [s.name for s in subs]
        assert _sub_path(sub_name) in sub_names

    def test_delete_subscription(self, pubsub_publisher, pubsub_subscriber):
        topic_name = _name()
        sub_name = _name()
        pubsub_publisher.create_topic(name=_topic_path(topic_name))
        pubsub_subscriber.create_subscription(
            request={"name": _sub_path(sub_name), "topic": _topic_path(topic_name)}
        )
        pubsub_subscriber.delete_subscription(subscription=_sub_path(sub_name))
        subs = list(pubsub_subscriber.list_subscriptions(
            request={"project": f"projects/{_PROJECT}"}
        ))
        assert _sub_path(sub_name) not in [s.name for s in subs]


class TestPublishAndPull:
    def test_publish_and_pull(self, pubsub_publisher, pubsub_subscriber):
        from google.cloud.pubsub_v1.types import PubsubMessage

        topic_name = _name()
        sub_name = _name()
        pubsub_publisher.create_topic(name=_topic_path(topic_name))
        pubsub_subscriber.create_subscription(
            request={"name": _sub_path(sub_name), "topic": _topic_path(topic_name)}
        )

        # Publish
        data = b"Hello, Pub/Sub!"
        pubsub_publisher.publish(
            topic=_topic_path(topic_name),
            messages=[PubsubMessage(data=data)],
        )

        # Pull
        response = pubsub_subscriber.pull(
            request={"subscription": _sub_path(sub_name), "max_messages": 10}
        )
        assert len(response.received_messages) == 1
        received = response.received_messages[0]
        assert received.message.data == data

    def test_publish_multiple_messages(self, pubsub_publisher, pubsub_subscriber):
        from google.cloud.pubsub_v1.types import PubsubMessage

        topic_name = _name()
        sub_name = _name()
        pubsub_publisher.create_topic(name=_topic_path(topic_name))
        pubsub_subscriber.create_subscription(
            request={"name": _sub_path(sub_name), "topic": _topic_path(topic_name)}
        )

        messages = [PubsubMessage(data=f"msg-{i}".encode()) for i in range(3)]
        pubsub_publisher.publish(topic=_topic_path(topic_name), messages=messages)

        response = pubsub_subscriber.pull(
            request={"subscription": _sub_path(sub_name), "max_messages": 10}
        )
        assert len(response.received_messages) == 3

    def test_acknowledge(self, pubsub_publisher, pubsub_subscriber):
        from google.cloud.pubsub_v1.types import PubsubMessage

        topic_name = _name()
        sub_name = _name()
        pubsub_publisher.create_topic(name=_topic_path(topic_name))
        pubsub_subscriber.create_subscription(
            request={"name": _sub_path(sub_name), "topic": _topic_path(topic_name)}
        )

        pubsub_publisher.publish(
            topic=_topic_path(topic_name), messages=[PubsubMessage(data=b"ack me")]
        )
        response = pubsub_subscriber.pull(
            request={"subscription": _sub_path(sub_name), "max_messages": 1}
        )
        ack_id = response.received_messages[0].ack_id
        pubsub_subscriber.acknowledge(
            request={"subscription": _sub_path(sub_name), "ack_ids": [ack_id]}
        )

        # After ack, message should not reappear (ackable deleted)
        r2 = pubsub_subscriber.pull(
            request={"subscription": _sub_path(sub_name), "max_messages": 10}
        )
        assert len(r2.received_messages) == 0

    def test_fanout_to_multiple_subscriptions(self, pubsub_publisher, pubsub_subscriber):
        from google.cloud.pubsub_v1.types import PubsubMessage

        topic_name = _name()
        sub1_name = _name()
        sub2_name = _name()
        pubsub_publisher.create_topic(name=_topic_path(topic_name))
        pubsub_subscriber.create_subscription(
            request={"name": _sub_path(sub1_name), "topic": _topic_path(topic_name)}
        )
        pubsub_subscriber.create_subscription(
            request={"name": _sub_path(sub2_name), "topic": _topic_path(topic_name)}
        )

        pubsub_publisher.publish(
            topic=_topic_path(topic_name), messages=[PubsubMessage(data=b"broadcast")]
        )

        r1 = pubsub_subscriber.pull(
            request={"subscription": _sub_path(sub1_name), "max_messages": 1}
        )
        r2 = pubsub_subscriber.pull(
            request={"subscription": _sub_path(sub2_name), "max_messages": 1}
        )
        assert len(r1.received_messages) == 1
        assert len(r2.received_messages) == 1
        assert r1.received_messages[0].message.data == b"broadcast"
        assert r2.received_messages[0].message.data == b"broadcast"

    def test_publish_with_attributes(self, pubsub_publisher, pubsub_subscriber):
        from google.cloud.pubsub_v1.types import PubsubMessage

        topic_name = _name()
        sub_name = _name()
        pubsub_publisher.create_topic(name=_topic_path(topic_name))
        pubsub_subscriber.create_subscription(
            request={"name": _sub_path(sub_name), "topic": _topic_path(topic_name)}
        )

        pubsub_publisher.publish(
            topic=_topic_path(topic_name),
            messages=[PubsubMessage(data=b"with-attrs", attributes={"key": "value"})],
        )
        response = pubsub_subscriber.pull(
            request={"subscription": _sub_path(sub_name), "max_messages": 1}
        )
        assert len(response.received_messages) == 1
