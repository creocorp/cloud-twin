"""
Deterministic fake-data generator for Bedrock simulation.

All generation is seeded from (model_id, field_path, request_count) so
the same request always produces the same output — critical for stable tests.
"""

from __future__ import annotations

import random
from typing import Any, Optional

from cloudtwin.providers.aws.bedrock.models import SchemaNodeConfig, TextConfig

# ---------------------------------------------------------------------------
# Deterministic source material
# ---------------------------------------------------------------------------

_LIPSUM = [
    "lorem",
    "ipsum",
    "dolor",
    "sit",
    "amet",
    "consectetur",
    "adipiscing",
    "elit",
    "sed",
    "do",
    "eiusmod",
    "tempor",
    "incididunt",
    "ut",
    "labore",
    "et",
    "dolore",
    "magna",
    "aliqua",
    "enim",
    "ad",
    "minim",
    "veniam",
    "quis",
    "nostrud",
    "exercitation",
    "ullamco",
    "laboris",
    "nisi",
    "aliquip",
    "ex",
    "ea",
    "commodo",
    "consequat",
]

# Deterministic float values cycled by request count
_FLOATS = [0.42, 0.73, 0.17, 0.85, 0.61, 0.33, 0.94, 0.28, 0.55, 0.79]

# Deterministic integer values cycled by request count
_INTEGERS = [1, 7, 42, 13, 99, 5, 28, 64, 3, 17]


def _seed(model_id: str, field_path: str, request_count: int) -> int:
    return hash(f"{model_id}:{field_path}:{request_count}") & 0x7FFF_FFFF


class BedrockGenerator:
    """Produces fake text and structured data from a config-driven schema."""

    # ------------------------------------------------------------------
    # Text
    # ------------------------------------------------------------------

    def generate_text(
        self,
        model_id: str,
        request_count: int,
        config: Optional[TextConfig] = None,
    ) -> str:
        min_words = config.min_words if config else 5
        max_words = config.max_words if config else 15
        rng = random.Random(_seed(model_id, "text", request_count))
        count = rng.randint(min_words, max_words)
        return " ".join(rng.choice(_LIPSUM) for _ in range(count))

    # ------------------------------------------------------------------
    # Schema-driven structured data
    # ------------------------------------------------------------------

    def generate_from_schema(
        self,
        schema: SchemaNodeConfig,
        model_id: str,
        request_count: int,
        field_path: str = "",
    ) -> Any:
        t = schema.type
        rng = random.Random(_seed(model_id, field_path, request_count))

        if schema.enum:
            idx = (request_count - 1) % len(schema.enum)
            return schema.enum[idx]

        if t == "object":
            return {
                key: self.generate_from_schema(
                    node, model_id, request_count, f"{field_path}.{key}"
                )
                for key, node in schema.properties.items()
            }

        if t == "array":
            if schema.items is None:
                return []
            count = rng.randint(1, 3)
            return [
                self.generate_from_schema(
                    schema.items, model_id, request_count, f"{field_path}[{i}]"
                )
                for i in range(count)
            ]

        if t == "string":
            return " ".join(rng.choice(_LIPSUM) for _ in range(2))

        if t == "number":
            return _FLOATS[(request_count - 1) % len(_FLOATS)]

        if t == "integer":
            return _INTEGERS[(request_count - 1) % len(_INTEGERS)]

        if t == "boolean":
            return (request_count % 2) == 1

        # Unknown type — fall back to None
        return None

    # ------------------------------------------------------------------
    # Static
    # ------------------------------------------------------------------

    def generate_static(self, payload: dict) -> dict:
        return dict(payload)
