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
            "bedrock",  # must come before "s3" – see providers/aws/bedrock/handlers.py
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
    enabled: bool = True


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
    # Raw ``bedrock:`` section from cloudtwin.yml (parsed by the bedrock package)
    bedrock: dict = field(default_factory=dict)


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

    # --- azure ---
    azure_section = yaml_data.get("providers", {}).get("azure", {})
    blob_section = azure_section.get("blob", {})
    sb_section = azure_section.get("servicebus", {})
    azure = AzureConfig(
        services=azure_section.get(
            "services",
            ["blob", "servicebus", "queue", "eventgrid", "keyvault", "functions"],
        ),
        blob=AzureBlobConfig(
            account_name=_env(
                "CLOUDTWIN_AZURE_ACCOUNT",
                blob_section.get("account_name", "devstoreaccount1"),
            ),
        ),
        servicebus=AzureServiceBusConfig(
            namespace=_env(
                "CLOUDTWIN_AZURE_NAMESPACE",
                sb_section.get("namespace", "cloudtwin"),
            ),
        ),
    )

    # --- gcp ---
    gcp_section = yaml_data.get("providers", {}).get("gcp", {})
    gcp = GcpConfig(
        project=_env(
            "CLOUDTWIN_GCP_PROJECT",
            gcp_section.get("project", "cloudtwin-local"),
        ),
        services=gcp_section.get(
            "services",
            [
                "storage",
                "pubsub",
                "firestore",
                "cloudtasks",
                "secretmanager",
                "cloudfunctions",
            ],
        ),
    )

    # --- dashboard ---
    dash_section = yaml_data.get("dashboard", {})
    dash_enabled_env = _env("CLOUDTWIN_DASHBOARD_ENABLED")

    if dash_enabled_env is not None:
        dash_enabled = dash_enabled_env.lower() in ("1", "true", "yes")
    else:
        dash_enabled = dash_section.get("enabled", True)
    dashboard = DashboardConfig(
        enabled=dash_enabled,
    )

    # --- logging ---
    log_section = yaml_data.get("logging", {})
    logging = LoggingConfig(
        level=_env("CLOUDTWIN_LOG_LEVEL", log_section.get("level", "info")),
    )

    return Config(
        storage=storage,
        providers=ProvidersConfig(aws=aws, azure=azure, gcp=gcp),
        dashboard=dashboard,
        logging=logging,
        api_port=int(_env("CLOUDTWIN_API_PORT", yaml_data.get("api_port", 4793))),
        bedrock=yaml_data.get("bedrock", {}),
    )
