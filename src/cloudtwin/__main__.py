"""
Entry point: python -m cloudtwin  or  cloudtwin (via pyproject script)
"""

from __future__ import annotations

import uvicorn

from cloudtwin.config import load_config


def main():
    config = load_config()

    uvicorn.run(
        "cloudtwin.app:create_app",
        factory=True,
        host="0.0.0.0",
        port=config.api_port,
        log_level=config.logging.level.lower(),
        reload=False,
    )


if __name__ == "__main__":
    main()
