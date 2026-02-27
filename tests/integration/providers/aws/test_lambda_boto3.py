"""AWS Lambda integration tests using boto3."""

from __future__ import annotations

import json
import uuid

import pytest


def _name() -> str:
    return f"fn-{uuid.uuid4().hex[:8]}"


_FAKE_CODE = {"ZipFile": b"fakecode"}
_RUNTIME = "python3.11"
_HANDLER = "index.handler"
_ROLE = "arn:aws:iam::000000000000:role/test"


class TestCreateFunction:
    def test_create_returns_arn_and_name(self, lambda_client):
        name = _name()
        resp = lambda_client.create_function(
            FunctionName=name,
            Runtime=_RUNTIME,
            Role=_ROLE,
            Handler=_HANDLER,
            Code=_FAKE_CODE,
        )
        assert resp["FunctionName"] == name
        assert "FunctionArn" in resp
        assert resp["Runtime"] == _RUNTIME

    def test_create_idempotent(self, lambda_client):
        name = _name()
        r1 = lambda_client.create_function(
            FunctionName=name, Runtime=_RUNTIME, Role=_ROLE,
            Handler=_HANDLER, Code=_FAKE_CODE,
        )
        r2 = lambda_client.create_function(
            FunctionName=name, Runtime=_RUNTIME, Role=_ROLE,
            Handler=_HANDLER, Code=_FAKE_CODE,
        )
        assert r1["FunctionArn"] == r2["FunctionArn"]


class TestListAndGetFunction:
    def test_list_functions_includes_created(self, lambda_client):
        name = _name()
        lambda_client.create_function(
            FunctionName=name, Runtime=_RUNTIME, Role=_ROLE,
            Handler=_HANDLER, Code=_FAKE_CODE,
        )
        resp = lambda_client.list_functions()
        names = [f["FunctionName"] for f in resp["Functions"]]
        assert name in names

    def test_get_function_returns_config(self, lambda_client):
        name = _name()
        lambda_client.create_function(
            FunctionName=name, Runtime=_RUNTIME, Role=_ROLE,
            Handler=_HANDLER, Code=_FAKE_CODE,
        )
        resp = lambda_client.get_function(FunctionName=name)
        assert resp["Configuration"]["FunctionName"] == name
        assert resp["Configuration"]["Handler"] == _HANDLER

    def test_get_nonexistent_raises(self, lambda_client):
        with pytest.raises(lambda_client.exceptions.ResourceNotFoundException):
            lambda_client.get_function(FunctionName=f"nonexistent-{uuid.uuid4().hex}")


class TestUpdateAndInvoke:
    def test_update_function_code(self, lambda_client):
        name = _name()
        lambda_client.create_function(
            FunctionName=name, Runtime=_RUNTIME, Role=_ROLE,
            Handler=_HANDLER, Code=_FAKE_CODE,
        )
        resp = lambda_client.update_function_code(
            FunctionName=name, ZipFile=b"newcode"
        )
        assert resp["FunctionName"] == name

    def test_invoke_returns_200_and_echoes_payload(self, lambda_client):
        name = _name()
        lambda_client.create_function(
            FunctionName=name, Runtime=_RUNTIME, Role=_ROLE,
            Handler=_HANDLER, Code=_FAKE_CODE,
        )
        payload = json.dumps({"key": "value"})
        resp = lambda_client.invoke(FunctionName=name, Payload=payload)
        assert resp["StatusCode"] == 200
        body = json.loads(resp["Payload"].read())
        assert "key" in json.dumps(body)  # echo contains original key


class TestDeleteFunction:
    def test_delete_then_get_raises(self, lambda_client):
        name = _name()
        lambda_client.create_function(
            FunctionName=name, Runtime=_RUNTIME, Role=_ROLE,
            Handler=_HANDLER, Code=_FAKE_CODE,
        )
        lambda_client.delete_function(FunctionName=name)
        with pytest.raises(lambda_client.exceptions.ResourceNotFoundException):
            lambda_client.get_function(FunctionName=name)
