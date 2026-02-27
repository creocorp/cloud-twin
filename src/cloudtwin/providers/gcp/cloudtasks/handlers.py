"""GCP Cloud Tasks — HTTP handlers (REST/JSON)."""

from __future__ import annotations

import logging

from fastapi import APIRouter, Request
from fastapi.responses import JSONResponse, Response

from cloudtwin.core.errors import CloudTwinError, NotFoundError
from cloudtwin.providers.gcp.cloudtasks.service import CloudTasksService

log = logging.getLogger("cloudtwin.gcp.cloudtasks")


def make_router(service: CloudTasksService) -> APIRouter:
    router = APIRouter()

    @router.post("/v2/projects/{project}/locations/{location}/queues")
    async def create_queue(project: str, location: str, request: Request) -> JSONResponse:
        try:
            body = await request.json()
        except Exception:
            body = {}
        name = body.get("name", "").split("/")[-1]
        try:
            queue = await service.create_queue(project, location, name)
            return JSONResponse({"name": queue.full_name}, status_code=200)
        except CloudTwinError as exc:
            return JSONResponse({"error": exc.message}, status_code=exc.http_status)

    @router.get("/v2/projects/{project}/locations/{location}/queues")
    async def list_queues(project: str, location: str) -> JSONResponse:
        queues = await service.list_queues(project)
        return JSONResponse({"queues": [{"name": q.full_name} for q in queues]})

    @router.get("/v2/projects/{project}/locations/{location}/queues/{queue_name}")
    async def get_queue(project: str, location: str, queue_name: str) -> JSONResponse:
        try:
            queue = await service.get_queue(project, location, queue_name)
            return JSONResponse({"name": queue.full_name})
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)

    @router.delete("/v2/projects/{project}/locations/{location}/queues/{queue_name}")
    async def delete_queue(project: str, location: str, queue_name: str) -> Response:
        try:
            await service.delete_queue(project, location, queue_name)
            return Response(status_code=200)
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)

    @router.post("/v2/projects/{project}/locations/{location}/queues/{queue_name}/tasks")
    async def create_task(project: str, location: str, queue_name: str, request: Request) -> JSONResponse:
        try:
            body = await request.json()
        except Exception:
            body = {}
        payload = body.get("task", {})
        try:
            task = await service.create_task(project, location, queue_name, payload)
            return JSONResponse({"name": task.task_id, "state": task.state}, status_code=200)
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)

    @router.get("/v2/projects/{project}/locations/{location}/queues/{queue_name}/tasks")
    async def list_tasks(project: str, location: str, queue_name: str) -> JSONResponse:
        try:
            tasks = await service.list_tasks(project, location, queue_name)
            return JSONResponse({"tasks": [{"name": t.task_id, "state": t.state} for t in tasks]})
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)

    @router.delete("/v2/projects/{project}/locations/{location}/queues/{queue_name}/tasks/{task_id}")
    async def delete_task(project: str, location: str, queue_name: str, task_id: str) -> Response:
        await service.delete_task(task_id)
        return Response(status_code=200)

    return router
