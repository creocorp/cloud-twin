"""
SMTP relay adapter.

Forwards SES emails via an external SMTP server when configured.
"""

from __future__ import annotations

import logging
from email.mime.multipart import MIMEMultipart
from email.mime.text import MIMEText
from typing import Optional

from cloudtwin.config import SmtpConfig

log = logging.getLogger("cloudtwin.ses.smtp")


async def relay_email(
    config: SmtpConfig,
    source: str,
    destinations: list[str],
    subject: str,
    text_body: Optional[str],
    html_body: Optional[str],
) -> None:
    """Forward email via external SMTP if configured."""
    if not config.enabled:
        return

    import aiosmtplib

    msg = MIMEMultipart("alternative")
    msg["Subject"] = subject or ""
    msg["From"] = source
    msg["To"] = ", ".join(destinations)

    if text_body:
        msg.attach(MIMEText(text_body, "plain"))
    if html_body:
        msg.attach(MIMEText(html_body, "html"))

    try:
        await aiosmtplib.send(
            msg,
            hostname=config.host,
            port=config.port,
            username=config.username,
            password=config.password,
            use_tls=config.use_tls,
        )
        log.info("Email relayed via SMTP to %s", destinations)
    except Exception as exc:
        log.warning("SMTP relay failed: %s", exc)
