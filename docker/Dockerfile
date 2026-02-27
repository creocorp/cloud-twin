FROM python:3.11-slim

WORKDIR /app

# Install build tools for any native deps
RUN apt-get update && apt-get install -y --no-install-recommends gcc && rm -rf /var/lib/apt/lists/*

COPY pyproject.toml ./
COPY src/ ./src/

RUN pip install --no-cache-dir -e .

# Data volume for SQLite persistence
VOLUME ["/data"]

# Config volume (optional override)
VOLUME ["/config"]

EXPOSE 4793
EXPOSE 8787

CMD ["cloudtwin"]
