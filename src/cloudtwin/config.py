"""
Configuration system for CloudTwin.

Priority order (highest to lowest):
  1. Environment variables
  2. YAML configuration file
  3. Built-in defaults
"""

from __future__ import annotations

import os
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

import yaml

# ---------------------------------------------------------------------------
# Sub-configs
# ---------------------------------------------------------------------------


@dataclass
class SmtpConfig:
    host: Optional[str] = None
    port: int = 1025
    username: Optional[str] = None
    password: Optional[str] = None
    use_tls: bool = False

    @property
    def enabled(self) -> bool:
        return self.host is not None


@dataclass
class SesConfig:
    strict_verification: bool = False
    smtp: SmtpConfig = field(default_factory=SmtpConfig)


@dataclass
class AwsConfig:
    services: list[str] = field(
        default_factory=lambda: [
            "ses",
            "sns",
            "sqs",
            "lambda",
            "dynamodb",
            "secretsmanager",
            "s3",
        ]
    )
    ses: SesConfig = field(default_factory=SesConfig)


@dataclass
class AzureBlobConfig:
    account_name: str = "devstoreaccount1"
    # Azurite's well-known dev key — safe to hard-code here
    account_key: str = "Eby8vdM02xNOcqFlJdE1SWKvW4GS0IEJSVDMuoFSSjM4="


@dataclass
class AzureServiceBusConfig:
    namespace: str = "cloudtwin"


@dataclass
class AzureConfig:
    services: list[str] = field(
        default_factory=lambda: [
            "blob",
            "servicebus",
            "queue",
            "eventgrid",
            "keyvault",
            "functions",
        ]
    )
    blob: AzureBlobConfig = field(default_factory=AzureBlobConfig)
    servicebus: AzureServiceBusConfig = field(default_factory=AzureServiceBusConfig)


@dataclass
class GcpStorageConfig:
    pass  # No extra settings required beyond the project


@dataclass
class GcpPubsubConfig:
    pass  # No extra settings required beyond the project


@dataclass
class GcpConfig:
    project: str = "cloudtwin-local"
    services: list[str] = field(
        default_factory=lambda: [
            "storage",
            "pubsub",
            "firestore",
            "cloudtasks",
            "secretmanager",
            "cloudfunctions",
        ]
    )
    storage: GcpStorageConfig = field(default_factory=GcpStorageConfig)
    pubsub: GcpPubsubConfig = field(default_factory=GcpPubsubConfig)


@dataclass
class ProvidersConfig:
    aws: AwsConfig = field(default_factory=AwsConfig)
    azure: AzureConfig = field(default_factory=AzureConfig)
    gcp: GcpConfig = field(default_factory=GcpConfig)


@dataclass
class StorageConfig:
    mode: str = "sqlite"  # "sqlite" | "memory"
    path: str = "./data/cloudtwin.db"


@dataclass
class DashboardConfig:
    enabled: bool = False
    port: int = 8793


@dataclass
class LoggingConfig:
    level: str = "info"


@dataclass
class Config:
    storage: StorageConfig = field(default_factory=StorageConfig)
    providers: ProvidersConfig = field(default_factory=ProvidersConfig)
    dashboard: DashboardConfig = field(default_factory=DashboardConfig)
    logging: LoggingConfig = field(default_factory=LoggingConfig)
    api_port: int = 4793


# ---------------------------------------------------------------------------
# Loader
# ---------------------------------------------------------------------------


def _load_yaml(path: Path) -> dict:
    if not path.exists():
        return {}
    with open(path) as f:
        data = yaml.safe_load(f) or {}
    return data.get("cloudtwin", data)


def _env(key: str, default=None):
    return os.environ.get(key, default)


def load_config() -> Config:
    config_path = Path(_env("CLOUDTWIN_CONFIG_PATH", "/config/cloudtwin.yml"))
    yaml_data = _load_yaml(config_path)

    # --- storage ---
    storage_section = yaml_data.get("storage", {})
    storage = StorageConfig(
        mode=_env("CLOUDTWIN_STORAGE_MODE", storage_section.get("mode", "sqlite")),
        path=_env(
            "CLOUDTWIN_STORAGE_PATH", storage_section.get("path", "./data/cloudtwin.db")
        ),
    )

    # --- smtp ---
    smtp_section = (
        yaml_data.get("providers", {}).get("aws", {}).get("ses", {}).get("smtp", {})
    )
    smtp = SmtpConfig(
        host=_env("CLOUDTWIN_SMTP_HOST", smtp_section.get("host")),
        port=int(_env("CLOUDTWIN_SMTP_PORT", smtp_section.get("port", 1025))),
        username=_env("CLOUDTWIN_SMTP_USERNAME", smtp_section.get("username")),
        password=_env("CLOUDTWIN_SMTP_PASSWORD", smtp_section.get("password")),
        use_tls=_env(
            "CLOUDTWIN_SMTP_USE_TLS", str(smtp_section.get("use_tls", False))
        ).lower()
        in ("1", "true", "yes"),
    )

    # --- ses ---
    ses_section = yaml_data.get("providers", {}).get("aws", {}).get("ses", {})
    strict_env = _env("CLOUDTWIN_AWS_SES_STRICT_VERIFICATION")
    if strict_env is not None:
        strict = strict_env.lower() in ("1", "true", "yes")
    else:
        strict = ses_section.get("strict_verification", False)
    ses = SesConfig(strict_verification=strict, smtp=smtp)

    # --- aws ---
    aws_section = yaml_data.get("providers", {}).get("aws", {})
    aws = AwsConfig(
        services=aws_section.get(
            "services",
            ["ses", "sns", "sqs", "lambda", "dynamodb", "secretsmanager", "s3"],
        ),
        ses=ses,
    )

    # --- dashboard ---
    dash_section = yaml_data.get("dashboard", {})
    dash_enabled_env = _env("CLOUDTWIN_DASHBOARD_ENABLED")
    if dash_enabled_env is not None:
        dash_enabled = dash_enabled_env.lower() in ("1", "true", "yes")
    else:
        dash_enabled = dash_section.get("enabled", False)
    dashboard = DashboardConfig(
        enabled=dash_enabled,
        port=int(_env("CLOUDTWIN_DASHBOARD_PORT", dash_section.get("port", 8793))),
    )

    # --- logging ---
    log_section = yaml_data.get("logging", {})
    logging = LoggingConfig(
        level=_env("CLOUDTWIN_LOG_LEVEL", log_section.get("level", "info")),
    )

    return Config(
        storage=storage,
        providers=ProvidersConfig(aws=aws),
        dashboard=dashboard,
        logging=logging,
        api_port=int(_env("CLOUDTWIN_API_PORT", yaml_data.get("api_port", 4793))),
    )
