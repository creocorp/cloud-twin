"""AWS Secrets Manager integration tests using boto3."""

from __future__ import annotations

import uuid

import pytest


def _name() -> str:
    return f"sec-{uuid.uuid4().hex[:8]}"


class TestCreateSecret:
    def test_create_with_string_returns_arn_and_name(self, secretsmanager):
        name = _name()
        resp = secretsmanager.create_secret(Name=name, SecretString="s3cr3t")
        assert resp["Name"] == name
        assert "ARN" in resp

    def test_create_idempotent(self, secretsmanager):
        name = _name()
        r1 = secretsmanager.create_secret(Name=name, SecretString="v1")
        r2 = secretsmanager.create_secret(Name=name, SecretString="v2")
        assert r1["ARN"] == r2["ARN"]

    def test_create_without_value(self, secretsmanager):
        name = _name()
        resp = secretsmanager.create_secret(Name=name)
        assert resp["Name"] == name


class TestGetAndListSecrets:
    def test_get_secret_value(self, secretsmanager):
        name = _name()
        secretsmanager.create_secret(Name=name, SecretString="my-value")
        resp = secretsmanager.get_secret_value(SecretId=name)
        assert resp["SecretString"] == "my-value"

    def test_get_nonexistent_raises(self, secretsmanager):
        with pytest.raises(secretsmanager.exceptions.ResourceNotFoundException):
            secretsmanager.get_secret_value(SecretId=f"no-such-{uuid.uuid4().hex}")

    def test_list_secrets_includes_created(self, secretsmanager):
        name = _name()
        secretsmanager.create_secret(Name=name, SecretString="val")
        resp = secretsmanager.list_secrets()
        names = [s["Name"] for s in resp["SecretList"]]
        assert name in names

    def test_describe_secret(self, secretsmanager):
        name = _name()
        secretsmanager.create_secret(Name=name, SecretString="data")
        resp = secretsmanager.describe_secret(SecretId=name)
        assert resp["Name"] == name
        assert "ARN" in resp


class TestPutSecretValue:
    def test_put_updates_value(self, secretsmanager):
        name = _name()
        secretsmanager.create_secret(Name=name, SecretString="old")
        secretsmanager.put_secret_value(SecretId=name, SecretString="new")
        resp = secretsmanager.get_secret_value(SecretId=name)
        assert resp["SecretString"] == "new"

    def test_put_on_nonexistent_raises(self, secretsmanager):
        with pytest.raises(secretsmanager.exceptions.ResourceNotFoundException):
            secretsmanager.put_secret_value(
                SecretId=f"missing-{uuid.uuid4().hex}", SecretString="x"
            )


class TestDeleteSecret:
    def test_delete_then_get_raises(self, secretsmanager):
        name = _name()
        secretsmanager.create_secret(Name=name, SecretString="bye")
        secretsmanager.delete_secret(SecretId=name, ForceDeleteWithoutRecovery=True)
        with pytest.raises(secretsmanager.exceptions.ResourceNotFoundException):
            secretsmanager.get_secret_value(SecretId=name)
