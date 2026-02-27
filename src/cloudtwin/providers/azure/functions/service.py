"""Azure Functions — pure business logic."""

from __future__ import annotations

import json
import logging
import uuid
from datetime import datetime, timezone

from cloudtwin.core.errors import NotFoundError
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.persistence.models.azure.functions import AzureFunction, AzureFunctionInvocation
from cloudtwin.persistence.repositories.azure.functions import (
    AzureFunctionInvocationRepository,
    AzureFunctionRepository,
)

log = logging.getLogger("cloudtwin.azure.functions")


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class AzureFunctionsService:
    def __init__(
        self,
        function_repo: AzureFunctionRepository,
        invocation_repo: AzureFunctionInvocationRepository,
        telemetry: TelemetryEngine,
    ):
        self._functions = function_repo
        self._invocations = invocation_repo
        self._telemetry = telemetry

    async def create_function(self, app: str, name: str, code: str = "") -> AzureFunction:
        existing = await self._functions.get(app, name)
        if existing:
            return existing
        fn = AzureFunction(app=app, name=name, code=code, created_at=_now())
        saved = await self._functions.save(fn)
        await self._telemetry.emit("azure", "functions", "create_function", {"app": app, "name": name})
        return saved

    async def get_function(self, app: str, name: str) -> AzureFunction:
        fn = await self._functions.get(app, name)
        if not fn:
            raise NotFoundError(f"Function not found: {app}/{name}")
        return fn

    async def list_functions(self, app: str) -> list[AzureFunction]:
        return await self._functions.list_by_app(app)

    async def delete_function(self, app: str, name: str) -> None:
        fn = await self._functions.get(app, name)
        if not fn:
            raise NotFoundError(f"Function not found: {app}/{name}")
        await self._functions.delete(app, name)
        await self._telemetry.emit("azure", "functions", "delete_function", {"app": app, "name": name})

    async def invoke(self, app: str, name: str, payload: str = "{}") -> str:
        fn = await self._functions.get(app, name)
        if not fn:
            raise NotFoundError(f"Function not found: {app}/{name}")
        response = json.dumps({"status": 200, "body": json.loads(payload)})
        inv = AzureFunctionInvocation(
            app=app,
            function_name=name,
            invocation_id=str(uuid.uuid4()),
            payload=payload,
            response=response,
            created_at=_now(),
        )
        await self._invocations.save(inv)
        await self._telemetry.emit("azure", "functions", "invoke", {"app": app, "name": name})
        return response
