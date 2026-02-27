"""GCP Secret Manager — HTTP handlers (REST/JSON)."""

from __future__ import annotations

import base64
import logging

from fastapi import APIRouter, Request
from fastapi.responses import JSONResponse, Response

from cloudtwin.core.errors import CloudTwinError, NotFoundError
from cloudtwin.providers.gcp.secretmanager.service import GcpSecretManagerService

log = logging.getLogger("cloudtwin.gcp.secretmanager")


def make_router(service: GcpSecretManagerService) -> APIRouter:
    router = APIRouter()

    @router.post("/v1/projects/{project}/secrets")
    async def create_secret(project: str, request: Request) -> JSONResponse:
        try:
            body = await request.json()
        except Exception:
            body = {}
        name = body.get("secretId", "") or request.query_params.get("secretId", "")
        try:
            secret = await service.create_secret(project, name)
            return JSONResponse({"name": secret.full_name}, status_code=200)
        except CloudTwinError as exc:
            return JSONResponse({"error": exc.message}, status_code=exc.http_status)

    @router.get("/v1/projects/{project}/secrets")
    async def list_secrets(project: str) -> JSONResponse:
        secrets = await service.list_secrets(project)
        return JSONResponse({"secrets": [{"name": s.full_name} for s in secrets]})

    @router.delete("/v1/projects/{project}/secrets/{secret_name}")
    async def delete_secret(project: str, secret_name: str) -> Response:
        try:
            await service.delete_secret(project, secret_name)
            return Response(status_code=200)
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)

    @router.post("/v1/projects/{project}/secrets/{secret_name}:addVersion")
    async def add_secret_version(project: str, secret_name: str, request: Request) -> JSONResponse:
        try:
            body = await request.json()
        except Exception:
            body = {}
        raw = body.get("payload", {}).get("data", "")
        payload = base64.b64decode(raw) if raw else b""
        try:
            version = await service.add_secret_version(project, secret_name, payload)
            return JSONResponse({"name": f"{version.secret_full_name}/versions/{version.version_id}"})
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)

    @router.get("/v1/projects/{project}/secrets/{secret_name}/versions/{version_id}:access")
    async def access_secret_version(project: str, secret_name: str, version_id: str) -> JSONResponse:
        try:
            version = await service.access_secret_version(project, secret_name, version_id)
            return JSONResponse({
                "name": f"{version.secret_full_name}/versions/{version.version_id}",
                "payload": {"data": base64.b64encode(version.payload).decode()},
            })
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)

    return router
