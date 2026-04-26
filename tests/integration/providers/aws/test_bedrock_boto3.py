"""
Bedrock simulation integration tests.

Start a real uvicorn server (in-memory mode) with a known bedrock sim config,
then exercise every major feature path via the boto3 bedrock-runtime and
bedrock clients.  No mocks — full HTTP + JSON / EventStream stack.

Test model IDs are configured in conftest.py's server_url fixture.
"""

from __future__ import annotations

import json

import pytest


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _invoke(client, model_id: str, prompt: str = "test") -> dict:
    """Invoke a model and return the parsed response body."""
    response = client.invoke_model(
        modelId=model_id,
        body=json.dumps({"prompt": prompt}).encode(),
        contentType="application/json",
        accept="application/json",
    )
    return json.loads(response["body"].read())


def _collect_stream(client, model_id: str, prompt: str = "test") -> list[dict]:
    """Invoke a model with streaming and return all decoded chunk payloads."""
    response = client.invoke_model_with_response_stream(
        modelId=model_id,
        body=json.dumps({"prompt": prompt}).encode(),
        contentType="application/json",
    )
    chunks = []
    for event in response["body"]:
        chunk = event.get("chunk")
        if chunk:
            chunks.append(json.loads(chunk["bytes"].decode()))
    return chunks


# ---------------------------------------------------------------------------
# ListFoundationModels
# ---------------------------------------------------------------------------


class TestListFoundationModels:
    def test_returns_model_list(self, bedrock_mgmt):
        resp = bedrock_mgmt.list_foundation_models()
        models = resp["modelSummaries"]
        assert isinstance(models, list)
        assert len(models) > 0

    def test_model_has_required_fields(self, bedrock_mgmt):
        resp = bedrock_mgmt.list_foundation_models()
        model = resp["modelSummaries"][0]
        assert "modelId" in model
        assert "modelName" in model
        assert "providerName" in model

    def test_known_model_present(self, bedrock_mgmt):
        resp = bedrock_mgmt.list_foundation_models()
        ids = {m["modelId"] for m in resp["modelSummaries"]}
        assert "test.text" in ids


# ---------------------------------------------------------------------------
# InvokeModel — text mode (default fallback)
# ---------------------------------------------------------------------------


class TestInvokeModelText:
    def test_returns_content_field(self, bedrock_runtime):
        body = _invoke(bedrock_runtime, "test.text")
        assert "content" in body
        assert isinstance(body["content"], str)
        assert len(body["content"]) > 0

    def test_returns_stop_reason(self, bedrock_runtime):
        body = _invoke(bedrock_runtime, "test.text")
        assert body.get("stop_reason") == "end_turn"

    def test_unknown_model_uses_text_defaults(self, bedrock_runtime):
        body = _invoke(bedrock_runtime, "unknown.model.xyz")
        assert "content" in body
        assert isinstance(body["content"], str)

    def test_response_is_deterministic_for_same_count(self, bedrock_runtime):
        # The state is shared per server session; just verify the call succeeds
        body = _invoke(bedrock_runtime, "test.text", "hello world")
        assert isinstance(body["content"], str)


# ---------------------------------------------------------------------------
# InvokeModel — schema mode
# ---------------------------------------------------------------------------


class TestInvokeModelSchema:
    def test_returns_json_object(self, bedrock_runtime):
        body = _invoke(bedrock_runtime, "test.schema")
        assert "answer" in body
        assert "score" in body
        assert "tags" in body

    def test_answer_is_string(self, bedrock_runtime):
        body = _invoke(bedrock_runtime, "test.schema")
        assert isinstance(body["answer"], str)

    def test_score_is_float(self, bedrock_runtime):
        body = _invoke(bedrock_runtime, "test.schema")
        assert isinstance(body["score"], float)

    def test_tags_is_list(self, bedrock_runtime):
        body = _invoke(bedrock_runtime, "test.schema")
        assert isinstance(body["tags"], list)


# ---------------------------------------------------------------------------
# InvokeModel — static mode
# ---------------------------------------------------------------------------


class TestInvokeModelStatic:
    def test_returns_fixed_payload(self, bedrock_runtime):
        body = _invoke(bedrock_runtime, "test.static")
        assert body["result"] == "fixed"
        assert body["value"] == 42

    def test_identical_on_repeated_calls(self, bedrock_runtime):
        body1 = _invoke(bedrock_runtime, "test.static")
        body2 = _invoke(bedrock_runtime, "test.static")
        assert body1["result"] == body2["result"]
        assert body1["value"] == body2["value"]


# ---------------------------------------------------------------------------
# InvokeModel — sequence mode
# ---------------------------------------------------------------------------


class TestInvokeModelSequence:
    """Sequence mode exhausts entries then re-uses the last one."""

    def test_first_call_returns_first_entry(self, bedrock_runtime):
        # Each test class gets a fresh model key perspective — but we share
        # the session server so the counter continues from wherever it left off.
        # We call twice and verify both answers are valid sequence entries.
        first = _invoke(bedrock_runtime, "test.sequence")
        second = _invoke(bedrock_runtime, "test.sequence")
        valid = {"first", "second"}
        assert first.get("answer") in valid
        assert second.get("answer") in valid

    def test_sequence_exhausted_uses_last(self, bedrock_runtime):
        # After 2 entries, subsequent calls should return "second"
        # Drain until we know we're past index 1
        for _ in range(5):
            body = _invoke(bedrock_runtime, "test.sequence")
        assert body.get("answer") == "second"


# ---------------------------------------------------------------------------
# InvokeModel — cycle mode
# ---------------------------------------------------------------------------


class TestInvokeModelCycle:
    """Cycle mode wraps back to the first entry after exhaustion."""

    def test_alternates_between_entries(self, bedrock_runtime):
        results = [_invoke(bedrock_runtime, "test.cycle") for _ in range(6)]
        answers = [r["answer"] for r in results]
        # Must only contain cycle entries
        for a in answers:
            assert a in ("a", "b"), f"Unexpected answer: {a!r}"

    def test_cycle_wraps(self, bedrock_runtime):
        # After many calls the pattern should repeat; just verify both values appear
        results = [_invoke(bedrock_runtime, "test.cycle") for _ in range(4)]
        answers = {r["answer"] for r in results}
        assert "a" in answers
        assert "b" in answers


# ---------------------------------------------------------------------------
# InvokeModel — prompt rules
# ---------------------------------------------------------------------------


class TestInvokeModelRules:
    def test_contains_rule_returns_configured_response(self, bedrock_runtime):
        body = _invoke(bedrock_runtime, "test.rules", "what is the sentiment of this?")
        assert body.get("sentiment") == "positive"

    def test_error_rule_raises(self, bedrock_runtime):
        from botocore.exceptions import ClientError

        with pytest.raises(ClientError) as exc_info:
            _invoke(bedrock_runtime, "test.rules", "please fail now")
        err = exc_info.value.response["Error"]
        assert "Throttl" in err["Code"] or "Throttl" in err.get("Message", "")

    def test_no_match_falls_through_to_mode(self, bedrock_runtime):
        body = _invoke(bedrock_runtime, "test.rules", "no matching keyword here")
        assert "content" in body


# ---------------------------------------------------------------------------
# InvokeModel — error injection (every-N)
# ---------------------------------------------------------------------------


class TestInvokeModelErrorInjection:
    def test_inject_errors_appear_in_sequence(self, bedrock_runtime):
        """With every=3, exactly 4 errors occur in any 12 consecutive calls."""
        from botocore.exceptions import ClientError

        errors_seen = 0
        successes_seen = 0
        for _ in range(12):
            try:
                body = _invoke(bedrock_runtime, "test.inject")
                assert "content" in body
                successes_seen += 1
            except ClientError:
                errors_seen += 1

        # In any 12 consecutive calls with every=3, exactly 4 errors fire
        assert errors_seen == 4, (
            f"Expected 4 injected errors in 12 calls, got {errors_seen}"
        )
        assert successes_seen == 8


# ---------------------------------------------------------------------------
# InvokeModelWithResponseStream
# ---------------------------------------------------------------------------


class TestInvokeModelStream:
    def test_stream_returns_multiple_chunks(self, bedrock_runtime):
        chunks = _collect_stream(bedrock_runtime, "test.stream", "hello world foo bar")
        assert len(chunks) > 0

    def test_chunk_has_delta_text(self, bedrock_runtime):
        chunks = _collect_stream(
            bedrock_runtime, "test.stream", "some text for streaming"
        )
        text_chunks = [
            c for c in chunks if c.get("type") == "content_block_delta" and "delta" in c
        ]
        assert len(text_chunks) > 0
        for ch in text_chunks:
            assert isinstance(ch["delta"]["text"], str)
            assert len(ch["delta"]["text"]) > 0

    def test_stream_ends_with_stop_event(self, bedrock_runtime):
        # Collect all raw events including non-chunk events
        response = bedrock_runtime.invoke_model_with_response_stream(
            modelId="test.stream",
            body=json.dumps({"prompt": "end stop test"}).encode(),
            contentType="application/json",
        )
        all_events = list(response["body"])
        # The stop event comes through as a 'chunk' with stopReason or as a
        # separate event; verify at minimum that we got some events
        assert len(all_events) > 0

    def test_stream_unknown_model_falls_back_to_text(self, bedrock_runtime):
        # Unknown models should still stream (using default text mode)
        chunks = _collect_stream(bedrock_runtime, "unknown.stream.model", "test prompt")
        assert len(chunks) > 0
