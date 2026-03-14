"""
Per-model request counter state for Bedrock simulation.

Thread-safe using a standard threading.Lock so the counter can be
incremented from concurrent async handlers without data races.
"""

from __future__ import annotations

import threading
from typing import Optional


class BedrockState:
    """Holds request counters keyed by modelId.

    ``increment`` returns the new (1-based) count so callers can use it
    immediately for sequence / error-injection decisions.
    """

    def __init__(self) -> None:
        self._counters: dict[str, int] = {}
        self._lock = threading.Lock()

    def increment(self, model_id: str) -> int:
        """Atomically increment the counter for *model_id* and return it."""
        with self._lock:
            count = self._counters.get(model_id, 0) + 1
            self._counters[model_id] = count
            return count

    def get_count(self, model_id: str) -> int:
        """Return the current counter for *model_id* without incrementing."""
        with self._lock:
            return self._counters.get(model_id, 0)

    def reset(self, model_id: Optional[str] = None) -> None:
        """Reset counters. Resets all models when *model_id* is None."""
        with self._lock:
            if model_id is None:
                self._counters.clear()
            else:
                self._counters.pop(model_id, None)
