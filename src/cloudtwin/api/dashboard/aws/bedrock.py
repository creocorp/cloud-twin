"""Dashboard — /api/dashboard/aws/bedrock"""

from __future__ import annotations

from fastapi import APIRouter, Request

router = APIRouter()


@router.get("/aws/bedrock")
async def aws_bedrock(request: Request):
    sim_config = getattr(request.app.state, "bedrock_sim_config", None)
    bedrock_state = getattr(request.app.state, "bedrock_state", None)

    if sim_config is None:
        return {"models": []}

    models = [
        {
            "model_id": model_id,
            "model_name": m.name or model_id,
            "provider": m.provider or model_id.split(".")[0],
            "request_count": bedrock_state.get_count(model_id) if bedrock_state else 0,
        }
        for model_id, m in sim_config.models.items()
    ]
    return {"models": models}
