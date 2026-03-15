"""
Central Bedrock scenario engine.

Implements the response-resolution pipeline documented in bedrock_support.md:

  1. Resolve model config (fall back to global defaults for unknown models)
  2. Increment request counter
  3. Evaluate injected errors (every-N)
  4. Evaluate prompt rules (contains – first match wins)
  5. Evaluate sequence / cycle history
  6. Fall back to configured mode (text / schema / static)
  7. Attach latency and streaming config metadata

The engine is a pure-Python async class with no HTTP or serialisation
knowledge — all of that lives in handlers.py.
"""

from __future__ import annotations

import asyncio
import random
from dataclasses import dataclass
from typing import Any, Optional

from cloudtwin.providers.aws.bedrock.generator import BedrockGenerator
from cloudtwin.providers.aws.bedrock.models import (
    BedrockSimConfig,
    ErrorEntryConfig,
    ErrorInjectConfig,
    LatencyConfig,
    ModelSimConfig,
    ResponseEntry,
    StreamingConfig,
)
from cloudtwin.providers.aws.bedrock.state import BedrockState


@dataclass
class ResolvedResponse:
    """Internal resolved response produced by the scenario engine."""

    kind: str  # "text" | "json" | "error"
    body: Any  # str for text, dict for json, None for error
    model_id: str
    request_count: int
    source: str  # "error" | "rule" | "sequence" | "fallback"
    streaming_config: Optional[StreamingConfig] = None
    latency_config: Optional[LatencyConfig] = None
    error_type: Optional[str] = None
    error_message: Optional[str] = None


class ScenarioEngine:
    def __init__(
        self,
        sim_config: BedrockSimConfig,
        state: BedrockState,
        generator: BedrockGenerator,
    ) -> None:
        self._config = sim_config
        self._state = state
        self._generator = generator

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

    def _get_model_config(self, model_id: str) -> ModelSimConfig:
        if model_id in self._config.models:
            return self._config.models[model_id]
        # Unknown model — build a stub from global defaults
        return ModelSimConfig(mode=self._config.defaults.mode)

    def _effective_latency(
        self, model_config: ModelSimConfig
    ) -> Optional[LatencyConfig]:
        return model_config.latency or self._config.defaults.latency

    def _resolve_entry(
        self,
        entry: ResponseEntry,
        model_id: str,
        request_count: int,
        source: str,
    ) -> ResolvedResponse:
        if entry.error is not None:
            return ResolvedResponse(
                kind="error",
                body=None,
                model_id=model_id,
                request_count=request_count,
                source=source,
                error_type=entry.error.type,
                error_message=entry.error.message,
            )
        if entry.static is not None:
            return ResolvedResponse(
                kind="json",
                body=dict(entry.static),
                model_id=model_id,
                request_count=request_count,
                source=source,
            )
        # text entry (explicit or implied)
        text = self._generator.generate_text(model_id, request_count, entry.text)
        return ResolvedResponse(
            kind="text",
            body=text,
            model_id=model_id,
            request_count=request_count,
            source=source,
        )

    def _check_inject_error(
        self, model_config: ModelSimConfig, request_count: int
    ) -> Optional[ErrorInjectConfig]:
        for err in model_config.errors:
            if request_count % err.every == 0:
                return err
        return None

    def _check_rules(
        self, model_config: ModelSimConfig, prompt_text: str
    ) -> Optional[tuple[Optional[ResponseEntry], Optional[ErrorEntryConfig]]]:
        for rule in model_config.rules:
            if rule.contains and rule.contains in prompt_text:
                return (rule.response, rule.error)
        return None

    def _resolve_sequence(
        self,
        model_config: ModelSimConfig,
        model_id: str,
        request_count: int,
    ) -> Optional[ResolvedResponse]:
        seq = model_config.sequence
        if seq is None or not seq.responses:
            return None
        n = len(seq.responses)
        if seq.mode == "cycle":
            idx = (request_count - 1) % n
        else:  # "sequence" — pin to last entry once exhausted
            idx = min(request_count - 1, n - 1)
        return self._resolve_entry(seq.responses[idx], model_id, request_count, "sequence")

    def _fallback(
        self,
        model_config: ModelSimConfig,
        model_id: str,
        request_count: int,
    ) -> ResolvedResponse:
        mode = model_config.mode or self._config.defaults.mode

        if mode == "static" and model_config.static is not None:
            return ResolvedResponse(
                kind="json",
                body=self._generator.generate_static(model_config.static),
                model_id=model_id,
                request_count=request_count,
                source="fallback",
            )

        if mode == "schema" and model_config.schema_config is not None:
            body = self._generator.generate_from_schema(
                model_config.schema_config, model_id, request_count
            )
            return ResolvedResponse(
                kind="json",
                body=body,
                model_id=model_id,
                request_count=request_count,
                source="fallback",
            )

        # text (default)
        text = self._generator.generate_text(model_id, request_count, model_config.text)
        return ResolvedResponse(
            kind="text",
            body=text,
            model_id=model_id,
            request_count=request_count,
            source="fallback",
        )

    # ------------------------------------------------------------------
    # Public interface
    # ------------------------------------------------------------------

    async def resolve(self, model_id: str, prompt_text: str) -> ResolvedResponse:
        """Resolve a response for the given model and prompt.

        Increments the model's request counter before applying any rules.
        """
        model_config = self._get_model_config(model_id)
        request_count = self._state.increment(model_id)

        # ---- 1. Injected errors ----------------------------------------
        inject_err = self._check_inject_error(model_config, request_count)
        if inject_err is not None:
            result = ResolvedResponse(
                kind="error",
                body=None,
                model_id=model_id,
                request_count=request_count,
                source="error",
                error_type=inject_err.type,
                error_message=inject_err.message,
            )
            result.latency_config = self._effective_latency(model_config)
            return result

        # ---- 2. Prompt rules -------------------------------------------
        rule_match = self._check_rules(model_config, prompt_text)
        if rule_match is not None:
            rule_response, rule_error = rule_match
            if rule_error is not None:
                result = ResolvedResponse(
                    kind="error",
                    body=None,
                    model_id=model_id,
                    request_count=request_count,
                    source="rule",
                    error_type=rule_error.type,
                    error_message=rule_error.message,
                )
            else:
                result = self._resolve_entry(
                    rule_response or ResponseEntry(),
                    model_id,
                    request_count,
                    "rule",
                )
            result.streaming_config = model_config.streaming
            result.latency_config = self._effective_latency(model_config)
            return result

        # ---- 3. Sequence / cycle ----------------------------------------
        seq_result = self._resolve_sequence(model_config, model_id, request_count)
        if seq_result is not None:
            seq_result.streaming_config = model_config.streaming
            seq_result.latency_config = self._effective_latency(model_config)
            return seq_result

        # ---- 4. Fallback ------------------------------------------------
        result = self._fallback(model_config, model_id, request_count)
        result.streaming_config = model_config.streaming
        result.latency_config = self._effective_latency(model_config)
        return result

    async def apply_latency(self, latency: Optional[LatencyConfig]) -> None:
        """Sleep for a random duration within the configured latency range."""
        if latency is None or (latency.min_ms == 0 and latency.max_ms == 0):
            return
        rng = random.Random()
        delay_ms = rng.uniform(latency.min_ms, latency.max_ms)
        await asyncio.sleep(delay_ms / 1000.0)
