"""Dashboard — /api/dashboard/azure/functions"""

from __future__ import annotations

from fastapi import APIRouter, Request

router = APIRouter()


@router.get("/azure/functions")
async def azure_functions(request: Request):
    repos = request.app.state.repos
    functions = await repos["azure_function"].list_all()
    return {
        "functions": [
            {
                "app": f.app,
                "name": f.name,
                "created_at": f.created_at,
            }
            for f in functions
        ]
    }
