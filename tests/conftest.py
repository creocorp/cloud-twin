"""
Black-box integration tests for the CloudTwin binary.

The session fixture ``cloudtwin_url`` starts ``target/debug/cloudtwin`` (built
with ``cargo build`` if missing) on a free port with in-memory storage and the
test config in ``tests/cloudtwin-test.yml``. Every suite drives it through the
official SDKs (boto3, Azure, GCP) or plain HTTP.

Environment:
    CLOUDTWIN_TEST_URL  Use an already-running server instead of spawning one.
    CLOUDTWIN_BIN       Path to the binary to spawn (default: target/debug/cloudtwin).
"""

from __future__ import annotations

import os
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import httpx
import pytest

ROOT = Path(__file__).resolve().parent.parent
TEST_CONFIG = Path(__file__).resolve().parent / "cloudtwin-test.yml"


def _free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def wait_ready(base_url: str, attempts: int = 100, delay: float = 0.1) -> None:
    for _ in range(attempts):
        try:
            if httpx.get(f"{base_url}/_health", timeout=1.0).status_code == 200:
                return
        except Exception:
            pass
        time.sleep(delay)
    raise RuntimeError(f"CloudTwin never became ready at {base_url}")


def _binary() -> Path:
    explicit = os.getenv("CLOUDTWIN_BIN")
    if explicit:
        return Path(explicit)
    path = ROOT / "target" / "debug" / "cloudtwin"
    if not path.exists():
        subprocess.run(["cargo", "build"], cwd=ROOT, check=True)
    return path


@pytest.fixture(scope="session")
def cloudtwin_url():
    external = os.getenv("CLOUDTWIN_TEST_URL")
    if external:
        wait_ready(external)
        yield external.rstrip("/")
        return

    port = _free_port()
    base_url = f"http://127.0.0.1:{port}"
    db_path = ":memory:"
    if os.getenv("CLOUDTWIN_STORAGE_MODE") == "sqlite":
        db_path = str(Path(tempfile.mkdtemp(prefix="cloudtwin_test_")) / "test.db")
    env = {
        **os.environ,
        "CLOUDTWIN_CONFIG_PATH": str(TEST_CONFIG),
        "CLOUDTWIN_PORT": str(port),
        "CLOUDTWIN_BASE_URL": base_url,
        "CLOUDTWIN_DB_PATH": db_path,
        "CLOUDTWIN_MAILBOX_ENABLED": "false",
        "RUST_LOG": os.getenv("RUST_LOG", "warn"),
    }
    proc = subprocess.Popen(
        [str(_binary())], env=env, stdout=sys.stderr, stderr=sys.stderr
    )
    try:
        wait_ready(base_url)
        yield base_url
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()


@pytest.fixture(scope="session")
def server_url(cloudtwin_url):
    """Alias kept for the AWS suites."""
    return cloudtwin_url
