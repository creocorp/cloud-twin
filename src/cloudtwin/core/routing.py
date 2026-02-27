"""
Shared routing utilities.
"""

from __future__ import annotations

from fastapi import APIRouter


def make_router(**kwargs) -> APIRouter:
    """Convenience wrapper; providers can add shared middleware here later."""
    return APIRouter(**kwargs)
