"""Dashboard — /api/dashboard/aws/s3"""

from __future__ import annotations

from fastapi import APIRouter, Request

router = APIRouter()


@router.get("/aws/s3")
async def aws_s3(request: Request):
    repos = request.app.state.repos
    buckets = await repos["s3_bucket"].list_all()
    return {
        "buckets": [
            {
                "name": b.name,
                "created_at": b.created_at,
            }
            for b in buckets
        ]
    }
