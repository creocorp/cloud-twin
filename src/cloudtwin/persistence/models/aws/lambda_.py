"""AWS Lambda domain model dataclasses."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional


@dataclass
class LambdaFunction:
    name: str
    arn: str
    runtime: str
    handler: str
    code: str        # base64-encoded zip or inline source
    created_at: str
    id: Optional[int] = None


@dataclass
class LambdaInvocation:
    function_name: str
    invocation_id: str
    payload: str     # JSON input
    response: str    # JSON output
    created_at: str
    id: Optional[int] = None
