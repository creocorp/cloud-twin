"""
CloudTwin CLI entry point.

Usage:
    cloudtwin terraform apply
    cloudtwin terraform plan
    cloudtwin server start
    cloudtwin server stop
    cloudtwin aws s3 ls
"""

from __future__ import annotations

import os
import subprocess
import sys
from typing import Annotated

import typer

from cli.commands import terraform as tf_mod
from cli.commands import server as server_mod

app = typer.Typer(
    name="cloudtwin",
    help="CloudTwin local cloud runtime CLI.",
    add_completion=False,
    no_args_is_help=True,
)

# Sub-command groups
app.add_typer(tf_mod.app,     name="terraform")
app.add_typer(server_mod.app, name="server")

_DEFAULT_URL = "http://localhost:4793"


@app.command(
    "aws",
    context_settings={"allow_extra_args": True, "ignore_unknown_options": True},
    add_help_option=False,
    help="Run any AWS CLI command pointed at CloudTwin.",
)
def aws_cmd(
    ctx: typer.Context,
    url: Annotated[
        str,
        typer.Option("--url", "-u", envvar="CLOUDTWIN_URL", help="CloudTwin base URL."),
    ] = _DEFAULT_URL,
) -> None:
    """
    Run any aws CLI command with --endpoint-url pre-set to CloudTwin.

    \b
    Examples:
      cloudtwin aws s3 ls
      cloudtwin aws s3 cp file.txt s3://my-bucket/file.txt
      cloudtwin aws ses list-identities
      cloudtwin aws sqs list-queues
    """
    aws_args = ctx.args
    if not aws_args:
        typer.echo(ctx.get_help())
        raise typer.Exit(0)

    env = {
        **os.environ,
        "AWS_ACCESS_KEY_ID":     "cloudtwin",
        "AWS_SECRET_ACCESS_KEY": "cloudtwin",
        "AWS_DEFAULT_REGION":    "us-east-1",
    }
    typer.echo(f"[cloudtwin] Running aws CLI with endpoint \u2192 {url}", err=True)
    result = subprocess.run(
        ["aws", "--endpoint-url", url, *aws_args],
        env=env,
    )
    sys.exit(result.returncode)


if __name__ == "__main__":
    app()