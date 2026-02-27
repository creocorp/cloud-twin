"""
Shared error types for CloudTwin.
"""

from __future__ import annotations


class CloudTwinError(Exception):
    """Base exception that maps directly to an HTTP response."""

    http_status: int = 500
    code: str = "InternalError"

    def __init__(self, message: str, code: str | None = None, http_status: int | None = None):
        super().__init__(message)
        self.message = message
        if code is not None:
            self.code = code
        if http_status is not None:
            self.http_status = http_status


class NotFoundError(CloudTwinError):
    http_status = 404
    code = "NoSuchEntity"

    def __init__(self, message: str = "Not found"):
        super().__init__(message)


class ValidationError(CloudTwinError):
    http_status = 400
    code = "ValidationError"

    def __init__(self, message: str):
        super().__init__(message)


class IdentityNotVerifiedError(CloudTwinError):
    http_status = 400
    code = "MessageRejected"

    def __init__(self, identity: str):
        super().__init__(f"Identity not verified: {identity}")


class ConflictError(CloudTwinError):
    http_status = 409
    code = "Conflict"

    def __init__(self, message: str = "Resource already exists"):
        super().__init__(message)
