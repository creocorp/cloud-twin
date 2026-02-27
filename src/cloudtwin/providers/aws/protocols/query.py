"""
AWS Protocol: Query (application/x-www-form-urlencoded)

Used by SES. Parses the Action field and routes to the correct handler.
"""

from __future__ import annotations

from typing import Callable, Awaitable
from fastapi import Request, Response


class QueryProtocolRouter:
    """
    Dispatcher for AWS Query-style requests.

    All requests arrive as POST with Content-Type
    application/x-www-form-urlencoded and an 'Action' field.
    """

    def __init__(self):
        self._handlers: dict[str, Callable[[Request, dict], Awaitable[Response]]] = {}

    def register(self, action: str, handler: Callable[[Request, dict], Awaitable[Response]]):
        self._handlers[action] = handler

    async def dispatch(self, request: Request) -> Response:
        form = await request.form()
        params = dict(form)
        action = params.get("Action")
        if not action:
            from fastapi.responses import Response as FR
            return FR(content=b"Missing Action", status_code=400)

        handler = self._handlers.get(action)
        if handler is None:
            from cloudtwin.core.xml import ses_error_response
            return Response(
                content=ses_error_response("InvalidAction", f"Unknown action: {action}"),
                status_code=400,
                media_type="text/xml",
            )
        return await handler(request, params)
