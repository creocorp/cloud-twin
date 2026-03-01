"""GCP Cloud Functions — HTTP handlers (REST/JSON)."""

from __future__ import annotations

import logging

from fastapi import APIRouter, Request
from fastapi.responses import JSONResponse, Response

from cloudtwin.core.errors import CloudTwinError, NotFoundError
from cloudtwin.providers.gcp.cloudfunctions.service import GcpCloudFunctionsService

log = logging.getLogger("cloudtwin.gcp.cloudfunctions")


def make_router(service: GcpCloudFunctionsService) -> APIRouter:
    router = APIRouter()

    @router.post("/v2/projects/{project}/locations/{location}/functions")
    async def create_function(
        project: str, location: str, request: Request
    ) -> JSONResponse:
        try:
            body = await request.json()
        except Exception:
            body = {}
        name = body.get("name", "").split("/")[-1] or request.query_params.get(
            "functionId", ""
        )
        runtime = body.get("buildConfig", {}).get("runtime", "python311")
        entrypoint = body.get("buildConfig", {}).get("entryPoint", "")
        try:
            fn = await service.create_function(
                project, location, name, runtime, entrypoint
            )
            return JSONResponse(
                {"name": fn.full_name, "runtime": fn.runtime}, status_code=200
            )
        except CloudTwinError as exc:
            return JSONResponse({"error": exc.message}, status_code=exc.http_status)

    @router.get("/v2/projects/{project}/locations/{location}/functions")
    async def list_functions(project: str, location: str) -> JSONResponse:
        fns = await service.list_functions(project)
        return JSONResponse(
            {"functions": [{"name": f.full_name, "runtime": f.runtime} for f in fns]}
        )

    @router.get("/v2/projects/{project}/locations/{location}/functions/{function_name}")
    async def get_function(
        project: str, location: str, function_name: str
    ) -> JSONResponse:
        try:
            fn = await service.get_function(project, location, function_name)
            return JSONResponse({"name": fn.full_name, "runtime": fn.runtime})
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)

    @router.delete(
        "/v2/projects/{project}/locations/{location}/functions/{function_name}"
    )
    async def delete_function(
        project: str, location: str, function_name: str
    ) -> Response:
        try:
            await service.delete_function(project, location, function_name)
            return Response(status_code=200)
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)

    @router.post(
        "/v2/projects/{project}/locations/{location}/functions/{function_name}:call"
    )
    async def invoke(
        project: str, location: str, function_name: str, request: Request
    ) -> JSONResponse:
        try:
            raw = await request.body()
            payload = raw.decode() if raw else "{}"
        except Exception:
            payload = "{}"
        try:
            result = await service.invoke(project, location, function_name, payload)
            return JSONResponse({"result": result})
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)
        except CloudTwinError as exc:
            return JSONResponse({"error": exc.message}, status_code=exc.http_status)

    return router
