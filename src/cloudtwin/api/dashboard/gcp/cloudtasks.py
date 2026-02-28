"""Dashboard — /api/dashboard/gcp/cloudtasks"""

from __future__ import annotations

from fastapi import APIRouter, Request

router = APIRouter()


@router.get("/gcp/cloudtasks")
async def gcp_cloudtasks(request: Request):
    config = request.app.state.config
    repos = request.app.state.repos
    project = config.providers.gcp.project
    queues = await repos["ct_queue"].list_by_project(project)
    result = []
    for q in queues:
        tasks = await repos["ct_task"].list_pending(q.full_name) if q.full_name else []
        result.append(
            {
                "name": q.full_name,
                "task_count": len(tasks),
                "created_at": q.created_at,
            }
        )
    return {"queues": result}
