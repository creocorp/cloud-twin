"""
Azure Service Bus integration tests using httpx directly.

The azure-servicebus SDK uses AMQP (not HTTP) for message operations,
so we test the HTTP REST API exposed by CloudTwin directly.
"""

from __future__ import annotations

import uuid

_NS = "cloudtwin-test"


def _q() -> str:
    return f"q-{uuid.uuid4().hex[:8]}"


def _t() -> str:
    return f"t-{uuid.uuid4().hex[:8]}"


class TestQueues:
    def test_create_queue(self, asb_http):
        q = _q()
        r = asb_http.put(f"/{_NS}/queues/{q}")
        assert r.status_code == 201
        assert r.json()["name"] == q

    def test_list_queues(self, asb_http):
        q = _q()
        asb_http.put(f"/{_NS}/queues/{q}")
        r = asb_http.get(f"/{_NS}/queues")
        assert r.status_code == 200
        names = [x["name"] for x in r.json()["queues"]]
        assert q in names

    def test_get_queue(self, asb_http):
        q = _q()
        asb_http.put(f"/{_NS}/queues/{q}")
        r = asb_http.get(f"/{_NS}/queues/{q}")
        assert r.status_code == 200
        assert r.json()["name"] == q

    def test_delete_queue(self, asb_http):
        q = _q()
        asb_http.put(f"/{_NS}/queues/{q}")
        r = asb_http.delete(f"/{_NS}/queues/{q}")
        assert r.status_code == 204
        r2 = asb_http.get(f"/{_NS}/queues/{q}")
        assert r2.status_code == 404

    def test_send_and_receive_message(self, asb_http):
        q = _q()
        asb_http.put(f"/{_NS}/queues/{q}")
        body = b"Hello from test!"
        r = asb_http.post(f"/{_NS}/queues/{q}/messages", content=body)
        assert r.status_code == 201

        r2 = asb_http.get(f"/{_NS}/queues/{q}/messages")
        assert r2.status_code == 200
        msgs = r2.json()["messages"]
        assert len(msgs) == 1
        assert msgs[0]["body"] == body.decode()
        assert msgs[0]["state"] == "locked"

    def test_complete_message(self, asb_http):
        q = _q()
        asb_http.put(f"/{_NS}/queues/{q}")
        asb_http.post(f"/{_NS}/queues/{q}/messages", content=b"msg")
        msgs = asb_http.get(f"/{_NS}/queues/{q}/messages").json()["messages"]
        lock_token = msgs[0]["lock_token"]

        r = asb_http.delete(f"/{_NS}/queues/{q}/messages/{lock_token}")
        assert r.status_code == 204

        # After completion, no more messages
        r2 = asb_http.get(f"/{_NS}/queues/{q}/messages")
        assert len(r2.json()["messages"]) == 0

    def test_abandon_message(self, asb_http):
        q = _q()
        asb_http.put(f"/{_NS}/queues/{q}")
        asb_http.post(f"/{_NS}/queues/{q}/messages", content=b"msg")
        msgs = asb_http.get(f"/{_NS}/queues/{q}/messages").json()["messages"]
        lock_token = msgs[0]["lock_token"]

        r = asb_http.post(f"/{_NS}/queues/{q}/messages/{lock_token}/abandon")
        assert r.status_code == 200

        # After abandon, message is back to active
        r2 = asb_http.get(f"/{_NS}/queues/{q}/messages")
        assert len(r2.json()["messages"]) == 1

    def test_receive_limit(self, asb_http):
        q = _q()
        asb_http.put(f"/{_NS}/queues/{q}")
        for i in range(5):
            asb_http.post(f"/{_NS}/queues/{q}/messages", content=f"msg-{i}".encode())
        r = asb_http.get(f"/{_NS}/queues/{q}/messages?limit=3")
        assert r.status_code == 200
        assert len(r.json()["messages"]) == 3


class TestTopicsAndSubscriptions:
    def test_create_topic(self, asb_http):
        t = _t()
        r = asb_http.put(f"/{_NS}/topics/{t}")
        assert r.status_code == 201
        assert r.json()["name"] == t

    def test_list_topics(self, asb_http):
        t = _t()
        asb_http.put(f"/{_NS}/topics/{t}")
        r = asb_http.get(f"/{_NS}/topics")
        assert r.status_code == 200
        names = [x["name"] for x in r.json()["topics"]]
        assert t in names

    def test_delete_topic(self, asb_http):
        t = _t()
        asb_http.put(f"/{_NS}/topics/{t}")
        r = asb_http.delete(f"/{_NS}/topics/{t}")
        assert r.status_code == 204

    def test_create_subscription(self, asb_http):
        t = _t()
        asb_http.put(f"/{_NS}/topics/{t}")
        r = asb_http.put(f"/{_NS}/topics/{t}/subscriptions/sub1")
        assert r.status_code == 201
        assert r.json()["name"] == "sub1"

    def test_list_subscriptions(self, asb_http):
        t = _t()
        asb_http.put(f"/{_NS}/topics/{t}")
        asb_http.put(f"/{_NS}/topics/{t}/subscriptions/sub-a")
        asb_http.put(f"/{_NS}/topics/{t}/subscriptions/sub-b")
        r = asb_http.get(f"/{_NS}/topics/{t}/subscriptions")
        assert r.status_code == 200
        names = [x["name"] for x in r.json()["subscriptions"]]
        assert "sub-a" in names
        assert "sub-b" in names

    def test_publish_and_receive_fanout(self, asb_http):
        t = _t()
        asb_http.put(f"/{_NS}/topics/{t}")
        asb_http.put(f"/{_NS}/topics/{t}/subscriptions/s1")
        asb_http.put(f"/{_NS}/topics/{t}/subscriptions/s2")

        r = asb_http.post(f"/{_NS}/topics/{t}/messages", content=b"broadcast!")
        assert r.status_code == 201
        assert r.json()["fan_out"] == 2

        r1 = asb_http.get(f"/{_NS}/topics/{t}/subscriptions/s1/messages")
        r2 = asb_http.get(f"/{_NS}/topics/{t}/subscriptions/s2/messages")
        assert len(r1.json()["messages"]) == 1
        assert len(r2.json()["messages"]) == 1
        assert r1.json()["messages"][0]["body"] == "broadcast!"
        assert r2.json()["messages"][0]["body"] == "broadcast!"

    def test_complete_subscription_message(self, asb_http):
        t = _t()
        asb_http.put(f"/{_NS}/topics/{t}")
        asb_http.put(f"/{_NS}/topics/{t}/subscriptions/s1")
        asb_http.post(f"/{_NS}/topics/{t}/messages", content=b"msg")

        msgs = asb_http.get(f"/{_NS}/topics/{t}/subscriptions/s1/messages").json()[
            "messages"
        ]
        lock_token = msgs[0]["lock_token"]
        r = asb_http.delete(f"/{_NS}/topics/{t}/subscriptions/s1/messages/{lock_token}")
        assert r.status_code == 204

        r2 = asb_http.get(f"/{_NS}/topics/{t}/subscriptions/s1/messages")
        assert len(r2.json()["messages"]) == 0
