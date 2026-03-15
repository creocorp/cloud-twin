"""
Bedrock REST HTTP handlers.

Mounts the following endpoints (all Bedrock Runtime paths):

  POST /model/{model_id}/invoke
      InvokeModel — non-streaming, returns JSON body

  POST /model/{model_id}/invoke-with-response-stream
      InvokeModelWithResponseStream — returns binary AWS EventStream

  GET  /foundation-models
      ListFoundationModels — returns a minimal list of known model stubs

The ``model_id`` parameter uses ``:path`` so that colons (e.g.
``anthropic.claude-3-sonnet-20240229-v1:0``) are captured correctly.

IMPORTANT: These routes must be registered BEFORE S3's wildcard
``/{bucket}`` route to prevent path-parameter shadowing.
"""

from __future__ import annotations

import json
import logging
import uuid
from typing import Any

from fastapi import APIRouter, Request, Response
from fastapi.responses import JSONResponse, StreamingResponse

from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.providers.aws.bedrock.models import StreamingConfig
from cloudtwin.providers.aws.bedrock.scenario_engine import ScenarioEngine
from cloudtwin.providers.aws.bedrock.streaming import stream_response

log = logging.getLogger("cloudtwin.bedrock")

# ---------------------------------------------------------------------------
# Minimal foundation-model catalogue returned by ListFoundationModels
# ---------------------------------------------------------------------------

_FOUNDATION_MODELS = [
    {
        "modelId": "anthropic.claude-3-sonnet-20240229-v1:0",
        "modelName": "Claude 3 Sonnet",
        "providerName": "Anthropic",
        "inputModalities": ["TEXT"],
        "outputModalities": ["TEXT"],
        "responseStreamingSupported": True,
    },
    {
        "modelId": "anthropic.claude-3-haiku-20240307-v1:0",
        "modelName": "Claude 3 Haiku",
        "providerName": "Anthropic",
        "inputModalities": ["TEXT"],
        "outputModalities": ["TEXT"],
        "responseStreamingSupported": True,
    },
    {
        "modelId": "meta.llama3-70b-instruct-v1:0",
        "modelName": "Llama 3 70B Instruct",
        "providerName": "Meta",
        "inputModalities": ["TEXT"],
        "outputModalities": ["TEXT"],
        "responseStreamingSupported": True,
    },
    {
        "modelId": "amazon.titan-text-express-v1",
        "modelName": "Titan Text Express",
        "providerName": "Amazon",
        "inputModalities": ["TEXT"],
        "outputModalities": ["TEXT"],
        "responseStreamingSupported": True,
    },
    {
        "modelId": "cohere.command-r-v1:0",
        "modelName": "Command R",
        "providerName": "Cohere",
        "inputModalities": ["TEXT"],
        "outputModalities": ["TEXT"],
        "responseStreamingSupported": True,
    },
]


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _extract_prompt(body: dict) -> str:
    """Best-effort extraction of user prompt text for rule matching."""
    for key in ("prompt", "inputText", "input"):
        val = body.get(key)
        if isinstance(val, str):
            return val
    messages = body.get("messages", [])
    if messages:
        last = messages[-1]
        if isinstance(last, dict):
            content = last.get("content", "")
            if isinstance(content, str):
                return content
            if isinstance(content, list):
                for block in content:
                    if isinstance(block, dict) and block.get("type") == "text":
                        return block.get("text", "")
    return json.dumps(body)


def _error_response(error_type: str, message: str, status: int = 400) -> Response:
    return JSONResponse(
        {"message": message},
        status_code=status,
        headers={"x-amzn-errortype": error_type},
    )


# ---------------------------------------------------------------------------
# Router factory
# ---------------------------------------------------------------------------

def make_bedrock_router(engine: ScenarioEngine, telemetry: TelemetryEngine) -> APIRouter:
    router = APIRouter()

    # ------------------------------------------------------------------
    # ListFoundationModels  GET /foundation-models
    # ------------------------------------------------------------------
    @router.get("/foundation-models")
    async def list_foundation_models(request: Request) -> Response:
        return JSONResponse({"modelSummaries": _FOUNDATION_MODELS})

    # ------------------------------------------------------------------
    # InvokeModel  POST /model/{model_id}/invoke
    # ------------------------------------------------------------------
    @router.post("/model/{model_id:path}/invoke")
    async def invoke_model(model_id: str, request: Request) -> Response:
        raw_body = await request.body()
        try:
            body: dict = json.loads(raw_body) if raw_body else {}
        except json.JSONDecodeError:
            return _error_response("ValidationException", "Request body is not valid JSON")

        prompt_text = _extract_prompt(body)

        try:
            resolved = await engine.resolve(model_id, prompt_text)
        except Exception as exc:  # pragma: no cover
            log.exception("Bedrock scenario engine error for model %r", model_id)
            return _error_response("InternalServerError", str(exc), 500)

        await engine.apply_latency(resolved.latency_config)

        if resolved.kind == "error":
            return _error_response(
                resolved.error_type or "ServiceException",
                resolved.error_message or "Synthetic error",
                400,
            )

        if resolved.kind == "text":
            response_body: Any = {
                "content": resolved.body,
                "stop_reason": "end_turn",
                "model": model_id,
            }
        else:
            response_body = resolved.body

        await telemetry.emit(
            "aws", "bedrock", "invoke_model",
            {"model_id": model_id, "request_count": resolved.request_count, "source": resolved.source},
        )

        return Response(
            content=json.dumps(response_body),
            media_type="application/json",
            headers={
                "x-amzn-requestid": str(uuid.uuid4()),
                "x-cloudtwin-request-count": str(resolved.request_count),
                "x-cloudtwin-response-source": resolved.source,
            },
        )

    # ------------------------------------------------------------------
    # InvokeModelWithResponseStream  POST /model/{model_id}/invoke-with-response-stream
    # ------------------------------------------------------------------
    @router.post("/model/{model_id:path}/invoke-with-response-stream")
    async def invoke_model_stream(model_id: str, request: Request) -> Response:
        raw_body = await request.body()
        try:
            body: dict = json.loads(raw_body) if raw_body else {}
        except json.JSONDecodeError:
            return _error_response("ValidationException", "Request body is not valid JSON")

        prompt_text = _extract_prompt(body)

        try:
            resolved = await engine.resolve(model_id, prompt_text)
        except Exception as exc:  # pragma: no cover
            log.exception("Bedrock stream error for model %r", model_id)
            return _error_response("InternalServerError", str(exc), 500)

        await engine.apply_latency(resolved.latency_config)

        if resolved.kind == "error":
            return _error_response(
                resolved.error_type or "ServiceException",
                resolved.error_message or "Synthetic error",
                400,
            )

        # Build streaming content
        if resolved.kind == "text":
            content = resolved.body
        else:
            content = json.dumps(resolved.body)

        stream_cfg = resolved.streaming_config or StreamingConfig(
            enabled=True, chunk_mode="word"
        )

        await telemetry.emit(
            "aws", "bedrock", "invoke_model_stream",
            {"model_id": model_id, "request_count": resolved.request_count, "source": resolved.source},
        )

        return StreamingResponse(
            stream_response(content, stream_cfg),
            media_type="application/vnd.amazon.eventstream",
            headers={
                "x-amzn-requestid": str(uuid.uuid4()),
                "x-cloudtwin-request-count": str(resolved.request_count),
                "transfer-encoding": "chunked",
            },
        )

    return router
