"""GCP Cloud Functions — pure business logic."""

from __future__ import annotations

import json
import logging
import uuid
from datetime import datetime, timezone

from cloudtwin.core.errors import NotFoundError
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.persistence.models.gcp.cloudfunctions import (
    CloudFunction,
    CloudFunctionInvocation,
)
from cloudtwin.persistence.repositories.gcp.cloudfunctions import (
    CloudFunctionInvocationRepository,
    CloudFunctionRepository,
)

log = logging.getLogger("cloudtwin.gcp.cloudfunctions")


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


def _full_name(project: str, location: str, name: str) -> str:
    return f"projects/{project}/locations/{location}/functions/{name}"


class GcpCloudFunctionsService:
    def __init__(
        self,
        function_repo: CloudFunctionRepository,
        invocation_repo: CloudFunctionInvocationRepository,
        telemetry: TelemetryEngine,
    ):
        self._functions = function_repo
        self._invocations = invocation_repo
        self._telemetry = telemetry

    async def create_function(
        self,
        project: str,
        location: str,
        name: str,
        runtime: str = "python311",
        entrypoint: str = "",
    ) -> CloudFunction:
        full_name = _full_name(project, location, name)
        existing = await self._functions.get(full_name)
        if existing:
            return existing
        fn = CloudFunction(
            project=project,
            name=name,
            full_name=full_name,
            runtime=runtime,
            entry_point=entrypoint,
            source_code="",
            created_at=_now(),
        )
        saved = await self._functions.save(fn)
        await self._telemetry.emit(
            "gcp",
            "cloudfunctions",
            "create_function",
            {"project": project, "name": name},
        )
        return saved

    async def get_function(
        self, project: str, location: str, name: str
    ) -> CloudFunction:
        fn = await self._functions.get(_full_name(project, location, name))
        if not fn:
            raise NotFoundError(f"Function not found: {name}")
        return fn

    async def list_functions(self, project: str) -> list[CloudFunction]:
        return await self._functions.list_by_project(project)

    async def delete_function(self, project: str, location: str, name: str) -> None:
        fn = await self._functions.get(_full_name(project, location, name))
        if not fn:
            raise NotFoundError(f"Function not found: {name}")
        await self._functions.delete(_full_name(project, location, name))
        await self._telemetry.emit(
            "gcp", "cloudfunctions", "delete_function", {"name": name}
        )

    async def invoke(
        self, project: str, location: str, name: str, payload: str = "{}"
    ) -> str:
        fn = await self._functions.get(_full_name(project, location, name))
        if not fn:
            raise NotFoundError(f"Function not found: {name}")
        response = json.dumps({"status": 200, "result": json.loads(payload)})
        inv = CloudFunctionInvocation(
            function_full_name=fn.full_name,
            invocation_id=str(uuid.uuid4()),
            payload=payload,
            response=response,
            created_at=_now(),
        )
        await self._invocations.save(inv)
        await self._telemetry.emit("gcp", "cloudfunctions", "invoke", {"name": name})
        return response
