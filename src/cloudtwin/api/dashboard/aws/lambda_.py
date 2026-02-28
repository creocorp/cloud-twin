"""Dashboard — /api/dashboard/aws/lambda"""

from __future__ import annotations

from fastapi import APIRouter, Request

router = APIRouter()


@router.get("/aws/lambda")
async def aws_lambda(request: Request):
    repos = request.app.state.repos
    functions = await repos["lambda_function"].list_all()
    return {
        "functions": [
            {
                "name": f.name,
                "runtime": f.runtime,
                "arn": f.arn,
                "created_at": f.created_at,
            }
            for f in functions
        ]
    }
