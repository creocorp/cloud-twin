"""Azure Functions domain model dataclasses."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional


@dataclass
class AzureFunction:
    app: str
    name: str
    code: str        # inline source or base64 zip
    created_at: str
    id: Optional[int] = None


@dataclass
class AzureFunctionInvocation:
    app: str
    function_name: str
    invocation_id: str
    payload: str
    response: str
    created_at: str
    id: Optional[int] = None
