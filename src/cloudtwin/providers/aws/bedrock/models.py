"""
Bedrock simulation config dataclasses.

Parsed from the ``bedrock:`` section of cloudtwin.yml (or a raw dict passed
at test time).  All parsing is handled here; the rest of the bedrock package
depends only on these types.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Optional


@dataclass
class LatencyConfig:
    min_ms: int = 0
    max_ms: int = 0

    @classmethod
    def from_dict(cls, data: dict) -> LatencyConfig:
        return cls(
            min_ms=int(data.get("min_ms", 0)),
            max_ms=int(data.get("max_ms", 0)),
        )


@dataclass
class StreamingConfig:
    enabled: bool = False
    chunk_mode: str = "word"  # "word" | "char" | "fixed_chars"
    fixed_chunk_size: int = 10
    first_chunk_delay_ms: int = 0
    chunk_delay_ms: int = 0
    fail_after_chunks: Optional[int] = None

    @classmethod
    def from_dict(cls, data: dict) -> StreamingConfig:
        return cls(
            enabled=bool(data.get("enabled", False)),
            chunk_mode=data.get("chunk_mode", "word"),
            fixed_chunk_size=int(data.get("fixed_chunk_size", 10)),
            first_chunk_delay_ms=int(data.get("first_chunk_delay_ms", 0)),
            chunk_delay_ms=int(data.get("chunk_delay_ms", 0)),
            fail_after_chunks=data.get("fail_after_chunks"),
        )


@dataclass
class SchemaNodeConfig:
    """Simplified JSON Schema node for recursive fake-data generation."""

    type: str = "string"
    properties: dict[str, SchemaNodeConfig] = field(default_factory=dict)
    items: Optional[SchemaNodeConfig] = None
    enum: Optional[list[Any]] = None

    @classmethod
    def from_dict(cls, data: dict) -> SchemaNodeConfig:
        props = {k: cls.from_dict(v) for k, v in data.get("properties", {}).items()}
        items_raw = data.get("items")
        return cls(
            type=data.get("type", "string"),
            properties=props,
            items=cls.from_dict(items_raw) if items_raw else None,
            enum=data.get("enum"),
        )


@dataclass
class TextConfig:
    template: str = "lipsum"
    min_words: int = 5
    max_words: int = 15

    @classmethod
    def from_dict(cls, data: dict) -> TextConfig:
        return cls(
            template=data.get("template", "lipsum"),
            min_words=int(data.get("min_words", 5)),
            max_words=int(data.get("max_words", 15)),
        )


@dataclass
class ErrorEntryConfig:
    type: str = "ThrottlingException"
    message: str = "Synthetic error"

    @classmethod
    def from_dict(cls, data: dict) -> ErrorEntryConfig:
        return cls(
            type=data.get("type", "ThrottlingException"),
            message=data.get("message", "Synthetic error"),
        )


@dataclass
class ResponseEntry:
    """One entry in a sequence list or a rule's response.

    Exactly one of ``static``, ``text``, or ``error`` should be set.
    """

    static: Optional[dict] = None
    mode: Optional[str] = None
    text: Optional[TextConfig] = None
    error: Optional[ErrorEntryConfig] = None

    @classmethod
    def from_dict(cls, data: dict) -> ResponseEntry:
        error_raw = data.get("error")
        text_raw = data.get("text")
        return cls(
            static=data.get("static"),
            mode=data.get("mode"),
            text=TextConfig.from_dict(text_raw) if text_raw else None,
            error=ErrorEntryConfig.from_dict(error_raw) if error_raw else None,
        )


@dataclass
class SequenceConfig:
    mode: str = "sequence"  # "sequence" | "cycle"
    responses: list[ResponseEntry] = field(default_factory=list)

    @classmethod
    def from_dict(cls, data: dict) -> SequenceConfig:
        return cls(
            mode=data.get("mode", "sequence"),
            responses=[ResponseEntry.from_dict(r) for r in data.get("responses", [])],
        )


@dataclass
class RuleConfig:
    """Prompt-based matching rule. First match wins."""

    contains: Optional[str] = None
    response: Optional[ResponseEntry] = None
    error: Optional[ErrorEntryConfig] = None

    @classmethod
    def from_dict(cls, data: dict) -> RuleConfig:
        response_raw = data.get("response")
        error_raw = data.get("error")
        return cls(
            contains=data.get("contains"),
            response=ResponseEntry.from_dict(response_raw) if response_raw else None,
            error=ErrorEntryConfig.from_dict(error_raw) if error_raw else None,
        )


@dataclass
class ErrorInjectConfig:
    """Periodic error injection. Triggers when ``request_count % every == 0``."""

    every: int = 5
    type: str = "ThrottlingException"
    message: str = "Injected error"

    @classmethod
    def from_dict(cls, data: dict) -> ErrorInjectConfig:
        return cls(
            every=int(data.get("every", 5)),
            type=data.get("type", "ThrottlingException"),
            message=data.get("message", "Injected error"),
        )


@dataclass
class ModelSimConfig:
    """Full simulation configuration for a single model ID."""

    mode: str = "text"  # "text" | "schema" | "static"
    static: Optional[dict] = None
    schema_config: Optional[SchemaNodeConfig] = None
    text: Optional[TextConfig] = None
    sequence: Optional[SequenceConfig] = None
    rules: list[RuleConfig] = field(default_factory=list)
    errors: list[ErrorInjectConfig] = field(default_factory=list)
    streaming: Optional[StreamingConfig] = None
    latency: Optional[LatencyConfig] = None

    @classmethod
    def from_dict(cls, data: dict) -> ModelSimConfig:
        schema_raw = data.get("schema")
        text_raw = data.get("text")
        seq_raw = data.get("sequence")
        stream_raw = data.get("streaming")
        latency_raw = data.get("latency")
        return cls(
            mode=data.get("mode", "text"),
            static=data.get("static"),
            schema_config=SchemaNodeConfig.from_dict(schema_raw) if schema_raw else None,
            text=TextConfig.from_dict(text_raw) if text_raw else None,
            sequence=SequenceConfig.from_dict(seq_raw) if seq_raw else None,
            rules=[RuleConfig.from_dict(r) for r in data.get("rules", [])],
            errors=[ErrorInjectConfig.from_dict(e) for e in data.get("errors", [])],
            streaming=StreamingConfig.from_dict(stream_raw) if stream_raw else None,
            latency=LatencyConfig.from_dict(latency_raw) if latency_raw else None,
        )


@dataclass
class BedrockDefaultsConfig:
    mode: str = "text"
    latency: Optional[LatencyConfig] = None

    @classmethod
    def from_dict(cls, data: dict) -> BedrockDefaultsConfig:
        latency_raw = data.get("latency")
        return cls(
            mode=data.get("mode", "text"),
            latency=LatencyConfig.from_dict(latency_raw) if latency_raw else None,
        )


@dataclass
class BedrockSimConfig:
    """Top-level parsed bedrock simulation config."""

    defaults: BedrockDefaultsConfig = field(default_factory=BedrockDefaultsConfig)
    models: dict[str, ModelSimConfig] = field(default_factory=dict)

    @classmethod
    def from_dict(cls, data: dict) -> BedrockSimConfig:
        return cls(
            defaults=BedrockDefaultsConfig.from_dict(data.get("defaults", {})),
            models={
                model_id: ModelSimConfig.from_dict(m)
                for model_id, m in data.get("models", {}).items()
            },
        )
