"""
AWS Bedrock simulation service package.

Entry point: register(app, config, repos, telemetry)

Bedrock simulation is stateless with respect to SQLite — all request
counters are held in memory for the lifetime of the process.  No
repository keys are consumed from *repos*.
"""

from __future__ import annotations

import logging

from fastapi import FastAPI

from cloudtwin.config import Config
from cloudtwin.core.telemetry import TelemetryEngine
from cloudtwin.providers.aws.bedrock.generator import BedrockGenerator
from cloudtwin.providers.aws.bedrock.handlers import make_bedrock_router
from cloudtwin.providers.aws.bedrock.models import BedrockSimConfig
from cloudtwin.providers.aws.bedrock.scenario_engine import ScenarioEngine
from cloudtwin.providers.aws.bedrock.state import BedrockState

log = logging.getLogger("cloudtwin.bedrock")


def register(
    app: FastAPI,
    config: Config,
    repos: dict,
    telemetry: TelemetryEngine,
) -> None:
    """Mount Bedrock REST endpoints onto *app*.

    NOTE: Must be registered **before** S3 to prevent S3's wildcard
    ``GET /{bucket}`` from shadowing ``GET /foundation-models``.
    """
    bedrock_raw: dict = getattr(config, "bedrock", {}) or {}
    sim_config = BedrockSimConfig.from_dict(bedrock_raw)
    state = BedrockState()
    generator = BedrockGenerator()
    engine = ScenarioEngine(sim_config, state, generator)

    app.include_router(make_bedrock_router(engine, telemetry))
    # Expose state and config for the dashboard API.
    app.state.bedrock_state = state
    app.state.bedrock_sim_config = sim_config
    log.info(
        "Bedrock simulation registered (%d model overrides configured)",
        len(sim_config.models),
    )
