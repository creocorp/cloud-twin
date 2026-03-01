"""AWS Lambda — pure business logic."""

from __future__ import annotations

import json
import logging
import uuid
from datetime import datetime, timezone

from cloudtwin.core.errors import NotFoundError
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.persistence.models.aws.lambda_ import LambdaFunction, LambdaInvocation
from cloudtwin.persistence.repositories.aws.lambda_ import (
    LambdaFunctionRepository,
    LambdaInvocationRepository,
)

log = logging.getLogger("cloudtwin.aws.lambda_")

_ACCOUNT_ID = "000000000000"
_REGION = "us-east-1"


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


def _arn(name: str) -> str:
    return f"arn:aws:lambda:{_REGION}:{_ACCOUNT_ID}:function:{name}"


class LambdaService:
    def __init__(
        self,
        function_repo: LambdaFunctionRepository,
        invocation_repo: LambdaInvocationRepository,
        telemetry: TelemetryEngine,
    ):
        self._functions = function_repo
        self._invocations = invocation_repo
        self._telemetry = telemetry

    async def create_function(
        self,
        name: str,
        runtime: str,
        handler: str,
        code: str = "",
    ) -> LambdaFunction:
        existing = await self._functions.get(name)
        if existing:
            return existing
        fn = LambdaFunction(
            name=name,
            arn=_arn(name),
            runtime=runtime,
            handler=handler,
            code=code,
            created_at=_now(),
        )
        saved = await self._functions.save(fn)
        await self._telemetry.emit("aws", "lambda", "create_function", {"name": name})
        return saved

    async def update_function_code(self, name: str, code: str) -> LambdaFunction:
        fn = await self._functions.get(name)
        if not fn:
            raise NotFoundError(f"Function not found: {name}")
        fn.code = code
        saved = await self._functions.save(fn)
        await self._telemetry.emit(
            "aws", "lambda", "update_function_code", {"name": name}
        )
        return saved

    async def get_function(self, name: str) -> LambdaFunction:
        fn = await self._functions.get(name)
        if not fn:
            raise NotFoundError(f"Function not found: {name}")
        return fn

    async def list_functions(self) -> list[LambdaFunction]:
        return await self._functions.list_all()

    async def delete_function(self, name: str) -> None:
        fn = await self._functions.get(name)
        if not fn:
            raise NotFoundError(f"Function not found: {name}")
        await self._functions.delete(name)
        await self._telemetry.emit("aws", "lambda", "delete_function", {"name": name})

    async def invoke(self, name: str, payload: str = "{}") -> str:
        fn = await self._functions.get(name)
        if not fn:
            raise NotFoundError(f"Function not found: {name}")
        # Stub invocation — echoes the payload back
        response = json.dumps({"StatusCode": 200, "Payload": json.loads(payload)})
        inv = LambdaInvocation(
            function_name=name,
            invocation_id=str(uuid.uuid4()),
            payload=payload,
            response=response,
            created_at=_now(),
        )
        await self._invocations.save(inv)
        await self._telemetry.emit("aws", "lambda", "invoke", {"name": name})
        return response
