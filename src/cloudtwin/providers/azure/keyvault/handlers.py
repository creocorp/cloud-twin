"""Azure Key Vault — HTTP handlers (REST/JSON)."""

from __future__ import annotations

import logging

from fastapi import APIRouter, Request
from fastapi.responses import JSONResponse, Response

from cloudtwin.core.errors import CloudTwinError, NotFoundError
from cloudtwin.providers.azure.keyvault.service import KeyVaultService

log = logging.getLogger("cloudtwin.azure.keyvault")


def make_router(service: KeyVaultService) -> APIRouter:
    router = APIRouter()

    @router.put("/azure/keyvault/{vault}/secrets/{secret_name}")
    async def set_secret(
        vault: str, secret_name: str, request: Request
    ) -> JSONResponse:
        try:
            body = await request.json()
        except Exception:
            return JSONResponse({"error": "Invalid JSON"}, status_code=400)
        try:
            secret = await service.set_secret(vault, secret_name, body.get("value", ""))
            return JSONResponse(
                {
                    "id": f"https://{vault}.vault.azure.net/secrets/{secret_name}/{secret.version}",
                    "value": secret.value,
                    "attributes": {"created": secret.created_at},
                }
            )
        except CloudTwinError as exc:
            return JSONResponse({"error": exc.message}, status_code=exc.http_status)

    @router.get("/azure/keyvault/{vault}/secrets/{secret_name}")
    async def get_secret(vault: str, secret_name: str) -> JSONResponse:
        try:
            secret = await service.get_secret(vault, secret_name)
            return JSONResponse(
                {
                    "id": f"https://{vault}.vault.azure.net/secrets/{secret_name}/{secret.version}",
                    "value": secret.value,
                    "attributes": {"created": secret.created_at},
                }
            )
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)

    @router.get("/azure/keyvault/{vault}/secrets/{secret_name}/{version}")
    async def get_secret_version(
        vault: str, secret_name: str, version: str
    ) -> JSONResponse:
        try:
            secret = await service.get_secret(vault, secret_name, version)
            return JSONResponse(
                {
                    "id": f"https://{vault}.vault.azure.net/secrets/{secret_name}/{secret.version}",
                    "value": secret.value,
                }
            )
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)

    @router.get("/azure/keyvault/{vault}/secrets")
    async def list_secrets(vault: str) -> JSONResponse:
        secrets = await service.list_secrets(vault)
        return JSONResponse(
            {
                "value": [
                    {
                        "id": f"https://{vault}.vault.azure.net/secrets/{s.name}",
                        "attributes": {},
                    }
                    for s in secrets
                ]
            }
        )

    @router.delete("/azure/keyvault/{vault}/secrets/{secret_name}")
    async def delete_secret(vault: str, secret_name: str) -> Response:
        try:
            await service.delete_secret(vault, secret_name)
            return Response(status_code=204)
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)

    return router
