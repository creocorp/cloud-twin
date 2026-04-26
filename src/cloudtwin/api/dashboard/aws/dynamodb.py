"""Dashboard — /api/dashboard/aws/dynamodb"""

from __future__ import annotations

from fastapi import APIRouter, Request

router = APIRouter()


@router.get("/aws/dynamodb")
async def aws_dynamodb(request: Request):
    repos = request.app.state.repos
    tables = await repos["dynamo_table"].list_all()
    return {
        "tables": [
            {
                "name": t.name,
                "status": "ACTIVE",
                "created_at": t.created_at,
            }
            for t in tables
        ]
    }
