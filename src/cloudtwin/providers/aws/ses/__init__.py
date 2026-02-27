"""AWS SES service package (v1 Query + v2 REST)."""

from __future__ import annotations

from fastapi import FastAPI, Request, Response

from cloudtwin.config import Config
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.persistence.repositories import SesIdentityRepository, SesMessageRepository
from cloudtwin.providers.aws.protocols.query import QueryProtocolRouter
from cloudtwin.providers.aws.ses.handlers import register_ses_handlers
from cloudtwin.providers.aws.ses.handlers_v2 import make_sesv2_router
from cloudtwin.providers.aws.ses.service import SesService


def register(
    app: FastAPI,
    config: Config,
    repos: dict,
    telemetry: TelemetryEngine,
    *,
    query_router=None,
    json_router=None,
) -> None:
    """Mount SES v1 (Query/XML) and v2 (REST/JSON) onto app."""
    ses_config = config.providers.aws.ses
    service = SesService(
        config=ses_config,
        identity_repo=repos["ses_identity"],
        message_repo=repos["ses_message"],
        telemetry=telemetry,
    )

    # v1 – AWS Query protocol (form-urlencoded, XML responses)
    # Register into the shared router if provided (multi-service mode),
    # otherwise create a standalone router and mount POST / directly.
    _query_router = query_router if query_router is not None else QueryProtocolRouter()
    register_ses_handlers(_query_router, ses_config, service)

    if query_router is None:
        # Standalone mode – SES owns the POST / endpoint
        @app.post("/")
        @app.post("")
        async def ses_endpoint(request: Request) -> Response:
            return await _query_router.dispatch(request)

    # v2 – REST/JSON (always mounted directly, no conflict with other services)
    app.include_router(make_sesv2_router(ses_config, service), prefix="/v2")


__all__ = ["register", "SesService"]
