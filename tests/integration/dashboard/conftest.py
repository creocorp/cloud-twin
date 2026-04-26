"""
conftest for dashboard integration tests.

Starts a real uvicorn server (Python backend) on a random port with the
dashboard API enabled and all AWS services registered, then yields the base URL.

To run against the Rust backend instead, set:

    CLOUDTWIN_TEST_URL=http://localhost:4793

and the fixture skips spinning up the Python server entirely.
"""

from __future__ import annotations

import os
import socket
import threading
import time
from pathlib import Path

import httpx
import pytest
import uvicorn
import yaml

from cloudtwin.app import create_app
from cloudtwin.config import (
    AwsConfig,
    Config,
    DashboardConfig,
    LoggingConfig,
    ProvidersConfig,
    SesConfig,
    SmtpConfig,
    StorageConfig,
)


def _load_bedrock_config() -> dict:
    """Load the bedrock: section from config/cloudtwin.yml."""
    repo_root = Path(__file__).parents[3]
    yml_path = repo_root / "config" / "cloudtwin.yml"
    if not yml_path.exists():
        return {}
    with open(yml_path) as f:
        data = yaml.safe_load(f) or {}
    top = data.get("cloudtwin", data)
    return top.get("bedrock", {})


def _free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _wait_ready(base_url: str, attempts: int = 60, delay: float = 0.1) -> None:
    for _ in range(attempts):
        try:
            r = httpx.get(f"{base_url}/_health", timeout=1.0)
            if r.status_code == 200:
                return
        except Exception:
            pass
        time.sleep(delay)
    raise RuntimeError(f"CloudTwin never became ready at {base_url}")


@pytest.fixture(scope="session")
def dashboard_url():
    """Base URL for /api/dashboard/* requests.

    Yields a URL that points at either:
    - the Python backend (spawned in-process on a random port), or
    - an external backend specified by CLOUDTWIN_TEST_URL.
    """
    external_url = os.getenv("CLOUDTWIN_TEST_URL")
    if external_url:
        _wait_ready(external_url)
        yield external_url
        return

    port = _free_port()
    config = Config(
        storage=StorageConfig(mode="memory"),
        providers=ProvidersConfig(
            aws=AwsConfig(
                services=["bedrock", "ses", "s3", "sns", "sqs"],
                ses=SesConfig(strict_verification=False, smtp=SmtpConfig()),
            )
        ),
        dashboard=DashboardConfig(enabled=True),
        logging=LoggingConfig(level="warning"),
        api_port=port,
        bedrock=_load_bedrock_config(),
    )
    app = create_app(config)
    server = uvicorn.Server(uvicorn.Config(app, host="127.0.0.1", port=port, log_level="warning"))
    thread = threading.Thread(target=server.run, daemon=True)
    thread.start()
    _wait_ready(f"http://127.0.0.1:{port}")
    yield f"http://127.0.0.1:{port}"
    server.should_exit = True
    thread.join(timeout=5)
