"""
Terraform passthrough command for CloudTwin.

Runs any terraform subcommand with environment variables pre-configured
to redirect AWS provider calls to the local CloudTwin instance, so your
Terraform scripts hit the local emulator instead of real AWS.

Usage examples:
    cloudtwin terraform init
    cloudtwin terraform plan
    cloudtwin terraform apply
    cloudtwin terraform apply --auto-approve
    cloudtwin terraform destroy
    cloudtwin terraform --url http://localhost:4793 apply
"""

from __future__ import annotations

import os
import subprocess
import sys
from typing import Annotated

import typer

app = typer.Typer(
    name="terraform",
    help="Run terraform commands pre-wired to CloudTwin.",
    add_completion=False,
)

_DEFAULT_URL = "http://localhost:4793"


def _cloudtwin_env(base_url: str) -> dict[str, str]:
    """
    Build the environment overlay that routes AWS SDK / Terraform AWS provider
    traffic to the CloudTwin instance at *base_url*.
    """
    return {
        # AWS SDK & Terraform AWS provider endpoint override
        "AWS_ENDPOINT_URL": base_url,
        # Terraform AWS provider uses these for STS/auth; any non-empty value works
        "AWS_ACCESS_KEY_ID":     "cloudtwin",
        "AWS_SECRET_ACCESS_KEY": "cloudtwin",
        "AWS_DEFAULT_REGION":    "us-east-1",
        # Skip TLS verification when hitting a plain HTTP local server
        "AWS_EC2_METADATA_DISABLED": "true",
        # Surface the endpoint as a TF variable so .tf files can reference it
        "TF_VAR_cloudtwin_endpoint": base_url,
        # Disable the remote Terraform backend by default (use local state)
        "TF_CLI_ARGS_init": "-backend=false",
    }


@app.command(
    context_settings={"allow_extra_args": True, "ignore_unknown_options": True},
    add_help_option=False,
)
def terraform(
    ctx: typer.Context,
    url: Annotated[
        str,
        typer.Option(
            "--url",
            "-u",
            envvar="CLOUDTWIN_URL",
            help="Base URL of the CloudTwin API server.",
            show_default=True,
        ),
    ] = _DEFAULT_URL,
    no_cloudtwin: Annotated[
        bool,
        typer.Option(
            "--no-cloudtwin",
            help="Pass through to terraform without modifying the environment.",
            is_flag=True,
        ),
    ] = False,
) -> None:
    """
    Run any terraform command with CloudTwin environment pre-configured.

    All positional arguments and flags after the options are forwarded to the
    terraform binary verbatim.

    \b
    Examples:
      cloudtwin terraform init
      cloudtwin terraform plan
      cloudtwin terraform apply --auto-approve
      cloudtwin terraform --url http://localhost:9000 apply
    """
    tf_args = ctx.args
    if not tf_args:
        typer.echo(ctx.get_help())
        raise typer.Exit(0)

    env = dict(os.environ)
    if not no_cloudtwin:
        env.update(_cloudtwin_env(url))
        typer.echo(
            f"[cloudtwin] Running terraform with AWS endpoint → {url}",
            err=True,
        )

    result = subprocess.run(["terraform", *tf_args], env=env)
    sys.exit(result.returncode)
