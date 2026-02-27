"""
AWS Protocol: JSON (application/x-amz-json-1.0)

Used by SQS. Parses the X-Amz-Target header and routes to the correct handler.
"""

from __future__ import annotations

import json
from typing import Awaitable, Callable

from fastapi import Request, Response
from fastapi.responses import JSONResponse


class JsonProtocolRouter:
    """
    Dispatcher for AWS JSON-style requests.

    All requests arrive as POST with Content-Type application/x-amz-json-1.0
    and an 'X-Amz-Target' header of the form 'Prefix.OperationName'.
    """

    def __init__(self):
        self._handlers: dict[str, Callable[[Request, dict], Awaitable[Response]]] = {}

    def register(self, target: str, handler: Callable[[Request, dict], Awaitable[Response]]):
        """Register a handler for the given X-Amz-Target value (e.g. 'AmazonSQS.SendMessage')."""
        self._handlers[target] = handler

    async def dispatch(self, request: Request) -> Response:
        target = request.headers.get("x-amz-target", "")
        if not target:
            return JSONResponse(
                {"__type": "InvalidAction", "message": "Missing X-Amz-Target header"},
                status_code=400,
            )

        handler = self._handlers.get(target)
        if handler is None:
            return JSONResponse(
                {"__type": "InvalidAction", "message": f"Unknown target: {target}"},
                status_code=400,
            )

        try:
            body_bytes = await request.body()
            body = json.loads(body_bytes) if body_bytes else {}
        except Exception:
            body = {}

        return await handler(request, body)
