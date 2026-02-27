"""AWS Lambda — HTTP handlers (REST/JSON protocol)."""

from __future__ import annotations

import logging

from fastapi import APIRouter, Request
from fastapi.responses import JSONResponse, Response

from cloudtwin.core.errors import CloudTwinError, NotFoundError
from cloudtwin.providers.aws.lambda_.service import LambdaService

log = logging.getLogger("cloudtwin.aws.lambda_")

_REGION = "us-east-1"
_ACCOUNT_ID = "000000000000"


def _fn_config(fn) -> dict:
    return {
        "FunctionName": fn.name,
        "FunctionArn": fn.arn,
        "Runtime": fn.runtime,
        "Handler": fn.handler,
        "LastModified": fn.created_at,
    }


def make_router(service: LambdaService) -> APIRouter:
    router = APIRouter()

    @router.post("/2015-03-31/functions")
    async def create_function(request: Request) -> JSONResponse:
        try:
            body = await request.json()
        except Exception:
            return JSONResponse({"message": "Invalid JSON"}, status_code=400)
        try:
            fn = await service.create_function(
                name=body["FunctionName"],
                runtime=body.get("Runtime", "python3.11"),
                handler=body.get("Handler", "index.handler"),
                code=str(body.get("Code", {})),
            )
            return JSONResponse(_fn_config(fn), status_code=201)
        except CloudTwinError as exc:
            return JSONResponse({"message": exc.message}, status_code=exc.http_status)

    @router.get("/2015-03-31/functions")
    async def list_functions(request: Request) -> JSONResponse:
        fns = await service.list_functions()
        return JSONResponse({"Functions": [_fn_config(f) for f in fns]})

    @router.get("/2015-03-31/functions/{function_name}")
    async def get_function(function_name: str) -> JSONResponse:
        try:
            fn = await service.get_function(function_name)
            return JSONResponse({"Configuration": _fn_config(fn)})
        except NotFoundError as exc:
            return JSONResponse({"__type": "ResourceNotFoundException", "message": exc.message}, status_code=404)

    @router.put("/2015-03-31/functions/{function_name}/code")
    async def update_function_code(function_name: str, request: Request) -> JSONResponse:
        try:
            body = await request.json()
        except Exception:
            return JSONResponse({"message": "Invalid JSON"}, status_code=400)
        try:
            fn = await service.update_function_code(function_name, str(body.get("ZipFile", "")))
            return JSONResponse(_fn_config(fn))
        except NotFoundError as exc:
            return JSONResponse({"__type": "ResourceNotFoundException", "message": exc.message}, status_code=404)

    @router.delete("/2015-03-31/functions/{function_name}")
    async def delete_function(function_name: str) -> Response:
        try:
            await service.delete_function(function_name)
            return Response(status_code=204)
        except NotFoundError as exc:
            return JSONResponse({"__type": "ResourceNotFoundException", "message": exc.message}, status_code=404)

    @router.post("/2015-03-31/functions/{function_name}/invocations")
    async def invoke_function(function_name: str, request: Request) -> JSONResponse:
        try:
            raw = await request.body()
            payload = raw.decode() if raw else "{}"
        except Exception:
            payload = "{}"
        try:
            result = await service.invoke(function_name, payload)
            return JSONResponse({"StatusCode": 200, "Payload": result})
        except NotFoundError as exc:
            return JSONResponse({"__type": "ResourceNotFoundException", "message": exc.message}, status_code=404)
        except CloudTwinError as exc:
            return JSONResponse({"message": exc.message}, status_code=exc.http_status)

    return router
