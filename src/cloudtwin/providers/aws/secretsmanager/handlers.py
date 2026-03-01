"""AWS Secrets Manager — HTTP handlers (JSON protocol, json_router dispatch)."""

from __future__ import annotations

import base64
import logging

from fastapi import Request, Response
from fastapi.responses import JSONResponse

from cloudtwin.core.errors import CloudTwinError, NotFoundError
from cloudtwin.providers.aws.protocols.json_protocol import JsonProtocolRouter
from cloudtwin.providers.aws.secretsmanager.service import SecretsManagerService

log = logging.getLogger("cloudtwin.aws.secretsmanager")

_PREFIX = "secretsmanager"


def _error(code: str, message: str, status: int = 400) -> JSONResponse:
    return JSONResponse({"__type": code, "message": message}, status_code=status)


def register_secretsmanager_handlers(
    router: JsonProtocolRouter, service: SecretsManagerService
) -> None:
    """Register all Secrets Manager JSON-protocol action handlers into the shared json_router."""

    async def create_secret(request: Request, body: dict) -> Response:
        try:
            secret = await service.create_secret(
                name=body["Name"],
                secret_string=body.get("SecretString"),
                secret_binary=(
                    base64.b64decode(body["SecretBinary"])
                    if body.get("SecretBinary")
                    else None
                ),
            )
        except CloudTwinError as exc:
            return _error(exc.code, exc.message, exc.http_status)
        return JSONResponse({"ARN": secret.arn, "Name": secret.name})

    async def get_secret_value(request: Request, body: dict) -> Response:
        try:
            version = await service.get_secret_value(
                name=body["SecretId"],
                version_id=body.get("VersionId"),
            )
        except NotFoundError as exc:
            return _error("ResourceNotFoundException", exc.message, 404)
        except CloudTwinError as exc:
            return _error(exc.code, exc.message, exc.http_status)
        resp: dict = {"Name": version.secret_name, "VersionId": version.version_id}
        if version.secret_string is not None:
            resp["SecretString"] = version.secret_string
        if version.secret_binary is not None:
            resp["SecretBinary"] = base64.b64encode(version.secret_binary).decode()
        return JSONResponse(resp)

    async def put_secret_value(request: Request, body: dict) -> Response:
        try:
            version = await service.put_secret_value(
                name=body["SecretId"],
                secret_string=body.get("SecretString"),
                secret_binary=(
                    base64.b64decode(body["SecretBinary"])
                    if body.get("SecretBinary")
                    else None
                ),
            )
        except NotFoundError as exc:
            return _error("ResourceNotFoundException", exc.message, 404)
        except CloudTwinError as exc:
            return _error(exc.code, exc.message, exc.http_status)
        return JSONResponse(
            {"ARN": version.secret_name, "VersionId": version.version_id}
        )

    async def describe_secret(request: Request, body: dict) -> Response:
        try:
            secret = await service.describe_secret(body["SecretId"])
        except NotFoundError as exc:
            return _error("ResourceNotFoundException", exc.message, 404)
        except CloudTwinError as exc:
            return _error(exc.code, exc.message, exc.http_status)
        return JSONResponse(
            {"ARN": secret.arn, "Name": secret.name, "CreatedDate": secret.created_at}
        )

    async def list_secrets(request: Request, body: dict) -> Response:
        try:
            secrets = await service.list_secrets()
        except CloudTwinError as exc:
            return _error(exc.code, exc.message, exc.http_status)
        return JSONResponse(
            {"SecretList": [{"ARN": s.arn, "Name": s.name} for s in secrets]}
        )

    async def delete_secret(request: Request, body: dict) -> Response:
        try:
            await service.delete_secret(body["SecretId"])
        except NotFoundError as exc:
            return _error("ResourceNotFoundException", exc.message, 404)
        except CloudTwinError as exc:
            return _error(exc.code, exc.message, exc.http_status)
        return JSONResponse({})

    async def update_secret(request: Request, body: dict) -> Response:
        """UpdateSecret: update value if provided."""
        try:
            secret = await service.describe_secret(body["SecretId"])
            if body.get("SecretString") or body.get("SecretBinary"):
                await service.put_secret_value(
                    name=secret.name,
                    secret_string=body.get("SecretString"),
                    secret_binary=(
                        base64.b64decode(body["SecretBinary"])
                        if body.get("SecretBinary")
                        else None
                    ),
                )
        except NotFoundError as exc:
            return _error("ResourceNotFoundException", exc.message, 404)
        except CloudTwinError as exc:
            return _error(exc.code, exc.message, exc.http_status)
        return JSONResponse({"ARN": secret.arn, "Name": secret.name})

    router.register(f"{_PREFIX}.CreateSecret", create_secret)
    router.register(f"{_PREFIX}.GetSecretValue", get_secret_value)
    router.register(f"{_PREFIX}.PutSecretValue", put_secret_value)
    router.register(f"{_PREFIX}.DescribeSecret", describe_secret)
    router.register(f"{_PREFIX}.ListSecrets", list_secrets)
    router.register(f"{_PREFIX}.DeleteSecret", delete_secret)
    router.register(f"{_PREFIX}.UpdateSecret", update_secret)
