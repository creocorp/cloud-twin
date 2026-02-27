"""AWS DynamoDB integration tests using boto3."""

from __future__ import annotations

import uuid

import pytest


def _name() -> str:
    return f"tbl-{uuid.uuid4().hex[:8]}"


_KEY_SCHEMA = [{"AttributeName": "pk", "KeyType": "HASH"}]
_ATTR_DEFS = [{"AttributeName": "pk", "AttributeType": "S"}]


class TestCreateTable:
    def test_create_returns_description(self, dynamodb):
        name = _name()
        resp = dynamodb.create_table(
            TableName=name,
            KeySchema=_KEY_SCHEMA,
            AttributeDefinitions=_ATTR_DEFS,
            BillingMode="PAY_PER_REQUEST",
        )
        td = resp["TableDescription"]
        assert td["TableName"] == name
        assert td["TableStatus"] == "ACTIVE"

    def test_list_tables_includes_new(self, dynamodb):
        name = _name()
        dynamodb.create_table(
            TableName=name,
            KeySchema=_KEY_SCHEMA,
            AttributeDefinitions=_ATTR_DEFS,
            BillingMode="PAY_PER_REQUEST",
        )
        resp = dynamodb.list_tables()
        assert name in resp["TableNames"]

    def test_describe_table(self, dynamodb):
        name = _name()
        dynamodb.create_table(
            TableName=name,
            KeySchema=_KEY_SCHEMA,
            AttributeDefinitions=_ATTR_DEFS,
            BillingMode="PAY_PER_REQUEST",
        )
        resp = dynamodb.describe_table(TableName=name)
        assert resp["Table"]["TableName"] == name
        assert resp["Table"]["TableStatus"] == "ACTIVE"


class TestItems:
    def _make_table(self, dynamodb):
        name = _name()
        dynamodb.create_table(
            TableName=name,
            KeySchema=_KEY_SCHEMA,
            AttributeDefinitions=_ATTR_DEFS,
            BillingMode="PAY_PER_REQUEST",
        )
        return name

    def test_put_and_get_item(self, dynamodb):
        table = self._make_table(dynamodb)
        item = {"pk": {"S": "key-1"}, "value": {"S": "hello"}}
        dynamodb.put_item(TableName=table, Item=item)
        resp = dynamodb.get_item(TableName=table, Key={"pk": {"S": "key-1"}})
        assert "Item" in resp
        assert resp["Item"]["value"]["S"] == "hello"

    def test_get_nonexistent_item_returns_empty(self, dynamodb):
        table = self._make_table(dynamodb)
        resp = dynamodb.get_item(TableName=table, Key={"pk": {"S": "missing"}})
        assert "Item" not in resp

    def test_delete_item(self, dynamodb):
        table = self._make_table(dynamodb)
        dynamodb.put_item(TableName=table, Item={"pk": {"S": "del-me"}})
        dynamodb.delete_item(TableName=table, Key={"pk": {"S": "del-me"}})
        resp = dynamodb.get_item(TableName=table, Key={"pk": {"S": "del-me"}})
        assert "Item" not in resp

    def test_scan_returns_items(self, dynamodb):
        table = self._make_table(dynamodb)
        for i in range(3):
            dynamodb.put_item(TableName=table, Item={"pk": {"S": f"scan-{i}"}})
        resp = dynamodb.scan(TableName=table)
        assert resp["Count"] >= 3

    def test_query_by_key(self, dynamodb):
        table = self._make_table(dynamodb)
        dynamodb.put_item(TableName=table, Item={"pk": {"S": "qk"}, "v": {"S": "qv"}})
        resp = dynamodb.query(
            TableName=table,
            KeyConditionExpression="pk = :pk",
            ExpressionAttributeValues={":pk": {"S": "qk"}},
        )
        assert resp["Count"] >= 1


class TestDeleteTable:
    def test_delete_table_then_describe_raises(self, dynamodb):
        name = _name()
        dynamodb.create_table(
            TableName=name,
            KeySchema=_KEY_SCHEMA,
            AttributeDefinitions=_ATTR_DEFS,
            BillingMode="PAY_PER_REQUEST",
        )
        dynamodb.delete_table(TableName=name)
        with pytest.raises(Exception):
            dynamodb.describe_table(TableName=name)
