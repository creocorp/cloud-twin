FROM python:3.11-slim

WORKDIR /app

# Install build tools for any native deps
RUN apt-get update && apt-get install -y --no-install-recommends gcc && rm -rf /var/lib/apt/lists/*

COPY pyproject.toml README.md ./
COPY src/ ./src/

RUN pip install --no-cache-dir .

# Data volume for SQLite persistence
VOLUME ["/data"]

# Config volume (optional override)
VOLUME ["/config"]

EXPOSE 4793

CMD ["cloudtwin"]
