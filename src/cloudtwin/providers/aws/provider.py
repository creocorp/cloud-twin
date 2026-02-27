"""
AWS Provider.

Discovers enabled services by name, imports their package, and calls
each service's register(app, config, repos, telemetry, *, query_router, json_router).

The provider owns the single POST / endpoint and routes requests based on
Content-Type:
  - application/x-amz-json-1.0  → json_router  (SQS)
  - application/x-www-form-urlencoded (default) → query_router  (SES v1, SNS)

Adding a new Query-protocol service (e.g. SNS) requires only:
  1. Creating providers/aws/sns/__init__.py with register() that accepts query_router
  2. Adding "sns" to the services list in config and _SERVICE_REGISTRY below
Adding a new JSON-protocol service (e.g. SQS) requires only:
  1. Creating providers/aws/sqs/__init__.py with register() that accepts json_router
  2. Adding "sqs" to the services list in config and _SERVICE_REGISTRY below
"""

from __future__ import annotations

import importlib
import logging

from fastapi import FastAPI, Request, Response

from cloudtwin.config import Config
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.persistence.repositories import make_repositories
from cloudtwin.providers.aws.protocols.json_protocol import JsonProtocolRouter
from cloudtwin.providers.aws.protocols.query import QueryProtocolRouter

log = logging.getLogger("cloudtwin.aws")

# Maps service name → importable package path
_SERVICE_REGISTRY: dict[str, str] = {
    "ses": "cloudtwin.providers.aws.ses",
    "s3": "cloudtwin.providers.aws.s3",
    "sns": "cloudtwin.providers.aws.sns",
    "sqs": "cloudtwin.providers.aws.sqs",
    "lambda": "cloudtwin.providers.aws.lambda_",
    "dynamodb": "cloudtwin.providers.aws.dynamodb",
    "secretsmanager": "cloudtwin.providers.aws.secretsmanager",
}


class AwsProvider:
    def __init__(self, config: Config, db):
        self._config = config
        self._db = db
        self._repos = make_repositories(db, mode=config.storage.mode)
        self._telemetry = TelemetryEngine(self._repos.get("event"))

    def register(self, app: FastAPI) -> None:
        query_router = QueryProtocolRouter()
        json_router = JsonProtocolRouter()

        for service_name in self._config.providers.aws.services:
            module_path = _SERVICE_REGISTRY.get(service_name)
            if module_path is None:
                log.warning("Unknown AWS service %r – skipping", service_name)
                continue
            module = importlib.import_module(module_path)
            # S3 does not use the shared routers – it mounts its own REST routes
            if service_name == "s3":
                module.register(app, self._config, self._repos, self._telemetry)
            else:
                module.register(
                    app,
                    self._config,
                    self._repos,
                    self._telemetry,
                    query_router=query_router,
                    json_router=json_router,
                )
            log.info("Registered AWS service: %s", service_name)

        # Mount the single POST / that dispatches to Query (SES, SNS) or JSON (SQS)
        # based on the Content-Type header.
        @app.post("/")
        @app.post("")
        async def aws_query_or_json_endpoint(request: Request) -> Response:
            content_type = request.headers.get("content-type", "")
            if "application/x-amz-json" in content_type:
                return await json_router.dispatch(request)
            return await query_router.dispatch(request)


