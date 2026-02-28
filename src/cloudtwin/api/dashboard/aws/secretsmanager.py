"""Dashboard — /api/dashboard/aws/secretsmanager"""

from __future__ import annotations

from fastapi import APIRouter, Request

router = APIRouter()


@router.get("/aws/secretsmanager")
async def aws_secretsmanager(request: Request):
    repos = request.app.state.repos
    secrets = await repos["sm_secret"].list_all()
    return {
        "secrets": [
            {
                "name": s.name,
                "arn": s.arn,
                "created_at": s.created_at,
            }
            for s in secrets
        ]
    }
