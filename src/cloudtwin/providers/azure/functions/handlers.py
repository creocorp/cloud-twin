"""Azure Functions — HTTP handlers (REST/JSON)."""

from __future__ import annotations

import logging

from fastapi import APIRouter, Request
from fastapi.responses import JSONResponse, Response

from cloudtwin.core.errors import CloudTwinError, NotFoundError
from cloudtwin.providers.azure.functions.service import AzureFunctionsService

log = logging.getLogger("cloudtwin.azure.functions")


def make_router(service: AzureFunctionsService) -> APIRouter:
    router = APIRouter()

    @router.put("/azure/functions/{app}/functions/{function_name}")
    async def create_function(app: str, function_name: str, request: Request) -> JSONResponse:
        try:
            body = await request.json()
        except Exception:
            body = {}
        try:
            fn = await service.create_function(app, function_name, code=str(body.get("code", "")))
            return JSONResponse({"app": fn.app, "name": fn.name}, status_code=201)
        except CloudTwinError as exc:
            return JSONResponse({"error": exc.message}, status_code=exc.http_status)

    @router.get("/azure/functions/{app}/functions")
    async def list_functions(app: str) -> JSONResponse:
        fns = await service.list_functions(app)
        return JSONResponse({"value": [{"app": f.app, "name": f.name} for f in fns]})

    @router.get("/azure/functions/{app}/functions/{function_name}")
    async def get_function(app: str, function_name: str) -> JSONResponse:
        try:
            fn = await service.get_function(app, function_name)
            return JSONResponse({"app": fn.app, "name": fn.name})
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)

    @router.delete("/azure/functions/{app}/functions/{function_name}")
    async def delete_function(app: str, function_name: str) -> Response:
        try:
            await service.delete_function(app, function_name)
            return Response(status_code=204)
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)

    @router.post("/azure/functions/{app}/functions/{function_name}/invoke")
    async def invoke(app: str, function_name: str, request: Request) -> JSONResponse:
        try:
            raw = await request.body()
            payload = raw.decode() if raw else "{}"
        except Exception:
            payload = "{}"
        try:
            result = await service.invoke(app, function_name, payload)
            return JSONResponse({"result": result})
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)
        except CloudTwinError as exc:
            return JSONResponse({"error": exc.message}, status_code=exc.http_status)

    return router
