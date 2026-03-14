"""
AWS EventStream encoding and async chunk generator for Bedrock streaming.

``encode_event_stream_message`` produces the binary-framed message format that
botocore's EventStream parser expects.  ``stream_response`` is an async
generator that wraps an already-resolved body and emits it as a series of
binary EventStream frames — keeping streaming concern completely separate from
the scenario resolution logic.

EventStream binary layout (big-endian):
  ┌────────────────────────────────────────────┐
  │  total_length       (4 bytes)              │
  │  headers_length     (4 bytes)              │  ← prelude (8 bytes)
  │  prelude_crc        (4 bytes, CRC32)       │
  │  message_headers    (headers_length bytes) │
  │  message_payload    (variable)             │
  │  message_crc        (4 bytes, CRC32)       │
  └────────────────────────────────────────────┘

total_length = 16 + headers_length + payload_length
               (prelude=8 + prelude_crc=4 + msg_crc=4 = 16 fixed overhead)
"""

from __future__ import annotations

import asyncio
import base64
import binascii
import json
import struct
from typing import AsyncIterator

from cloudtwin.providers.aws.bedrock.models import StreamingConfig


# ---------------------------------------------------------------------------
# Binary EventStream encoding
# ---------------------------------------------------------------------------

def _encode_header_field(name: str, value: str) -> bytes:
    """Encode a single EventStream header name/value pair (string type = 7)."""
    name_b = name.encode("utf-8")
    value_b = value.encode("utf-8")
    return (
        struct.pack("!B", len(name_b))
        + name_b
        + bytes([7])  # header value type: string
        + struct.pack("!H", len(value_b))
        + value_b
    )


def encode_event_stream_message(headers: dict[str, str], payload: bytes) -> bytes:
    """Return a complete binary-framed EventStream message."""
    headers_data = b"".join(
        _encode_header_field(k, v) for k, v in headers.items()
    )
    # total = prelude(8) + prelude_crc(4) + headers + payload + msg_crc(4)
    total_length = 16 + len(headers_data) + len(payload)
    prelude = struct.pack("!II", total_length, len(headers_data))
    prelude_crc = binascii.crc32(prelude) & 0xFFFF_FFFF
    message_no_crc = prelude + struct.pack("!I", prelude_crc) + headers_data + payload
    message_crc = binascii.crc32(message_no_crc) & 0xFFFF_FFFF
    return message_no_crc + struct.pack("!I", message_crc)


# ---------------------------------------------------------------------------
# Bedrock-specific event builders
# ---------------------------------------------------------------------------

def _chunk_event(chunk_content: dict) -> bytes:
    """Encode one content chunk as a Bedrock ``chunk`` EventStream event."""
    payload = json.dumps(
        {"bytes": base64.b64encode(json.dumps(chunk_content).encode()).decode()}
    ).encode()
    return encode_event_stream_message(
        {
            ":message-type": "event",
            ":event-type": "chunk",
            ":content-type": "application/json",
        },
        payload,
    )


def _stop_event() -> bytes:
    """Encode the terminal Bedrock ``messageStop`` event."""
    payload = json.dumps({"stopReason": "end_turn"}).encode()
    return encode_event_stream_message(
        {
            ":message-type": "event",
            ":event-type": "messageStop",
            ":content-type": "application/json",
        },
        payload,
    )


def _error_event(error_type: str, message: str) -> bytes:
    """Encode a mid-stream error as an EventStream exception event."""
    payload = json.dumps({"message": message}).encode()
    return encode_event_stream_message(
        {
            ":message-type": "exception",
            ":exception-type": error_type,
            ":content-type": "application/json",
        },
        payload,
    )


# ---------------------------------------------------------------------------
# Chunk splitting
# ---------------------------------------------------------------------------

def split_chunks(text: str, config: StreamingConfig) -> list[str]:
    """Split *text* into chunks according to *config.chunk_mode*."""
    if config.chunk_mode == "word":
        parts = text.split()
        return parts if parts else [text]
    if config.chunk_mode == "char":
        return list(text) if text else [text]
    if config.chunk_mode == "fixed_chars":
        size = max(1, config.fixed_chunk_size)
        return [text[i: i + size] for i in range(0, max(1, len(text)), size)]
    return [text]


# ---------------------------------------------------------------------------
# Async generator
# ---------------------------------------------------------------------------

async def stream_response(
    content: str | dict,
    config: StreamingConfig,
) -> AsyncIterator[bytes]:
    """Async generator that yields binary EventStream frames for *content*.

    Applies ``first_chunk_delay_ms`` before the first chunk and
    ``chunk_delay_ms`` between subsequent chunks.  Emits an error event
    and returns early when ``fail_after_chunks`` is reached.
    """
    if isinstance(content, dict):
        text = json.dumps(content)
    else:
        text = str(content)

    chunks = split_chunks(text, config)
    fail_at = config.fail_after_chunks

    if config.first_chunk_delay_ms > 0:
        await asyncio.sleep(config.first_chunk_delay_ms / 1000.0)

    for i, chunk_text in enumerate(chunks):
        if fail_at is not None and i >= fail_at:
            yield _error_event("ModelStreamErrorException", "Synthetic stream failure")
            return

        chunk_content = {
            "type": "content_block_delta",
            "index": i,
            "delta": {"type": "text_delta", "text": chunk_text},
        }
        yield _chunk_event(chunk_content)

        if config.chunk_delay_ms > 0 and i < len(chunks) - 1:
            await asyncio.sleep(config.chunk_delay_ms / 1000.0)

    yield _stop_event()
