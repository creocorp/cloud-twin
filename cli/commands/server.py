"""
Server management commands — start/stop the CloudTwin API server.
"""

from __future__ import annotations

import subprocess
import sys

import typer

app = typer.Typer(
    name="server",
    help="Start or stop the CloudTwin API server.",
    add_completion=False,
)


@app.command()
def start(
    port: int = typer.Option(4793, "--port", "-p", help="API port."),
    dashboard: bool = typer.Option(False, "--dashboard", help="Enable the dashboard."),
    storage: str = typer.Option(
        "memory",
        "--storage",
        help="Storage mode: 'memory' or 'sqlite'.",
    ),
) -> None:
    """Start the CloudTwin API server."""

    env = {
        **__import__("os").environ,
        "CLOUDTWIN_API_PORT": str(port),
        "CLOUDTWIN_STORAGE_MODE": storage,
        "CLOUDTWIN_DASHBOARD_ENABLED": "true" if dashboard else "false",
    }
    typer.echo(f"[cloudtwin] Starting server on port {port} (storage={storage}) …")
    result = subprocess.run(
        [sys.executable, "-m", "cloudtwin"],
        env=env,
    )
    sys.exit(result.returncode)


@app.command()
def stop() -> None:
    """Stop any running CloudTwin server processes."""
    result = subprocess.run(
        ["pkill", "-f", "cloudtwin"],
        capture_output=True,
    )
    if result.returncode == 0:
        typer.echo("[cloudtwin] Server stopped.")
    else:
        typer.echo("[cloudtwin] No running server found.", err=True)
