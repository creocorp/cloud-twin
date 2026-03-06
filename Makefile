VENV        := .venv
PYTHON      := $(VENV)/bin/python
PIP         := $(VENV)/bin/pip
UVICORN     := $(VENV)/bin/uvicorn
PYTEST      := $(VENV)/bin/pytest

.DEFAULT_GOAL := help

# ── Bootstrap ──────────────────────────────────────────────────────────────────

$(VENV)/bin/activate:
	python3 -m venv $(VENV)
	$(PIP) install --upgrade pip

.PHONY: install
install: $(VENV)/bin/activate  ## Install runtime dependencies into venv
	$(PIP) install -e .

.PHONY: install-dev
install-dev: $(VENV)/bin/activate  ## Install all dependencies (runtime + dev) into venv
	$(PIP) install -e ".[dev]"

# ── Run ────────────────────────────────────────────────────────────────────────

.PHONY: run
run: install  ## Start the CloudTwin server (port 4793)
	mkdir -p data
	CLOUDTWIN_CONFIG_PATH=config/cloudtwin.yml CLOUDTWIN_STORAGE_PATH=data/cloudtwin.db $(UVICORN) cloudtwin.app:create_app --factory --host 0.0.0.0 --port 4793 --reload

.PHONY: run-with-dashboard
run-with-dashboard: install  ## Start CloudTwin with the dashboard enabled (http://localhost:4793/dashboard)
	mkdir -p data
	CLOUDTWIN_CONFIG_PATH=config/cloudtwin.yml CLOUDTWIN_STORAGE_PATH=data/cloudtwin.db CLOUDTWIN_DASHBOARD_ENABLED=true $(UVICORN) cloudtwin.app:create_app --factory --host 0.0.0.0 --port 4793 --reload

.PHONY: dashboard-dev
dashboard-dev:  ## Start the Vite dashboard dev server (proxies /api/* to port 4793)
	cd dashboard && npm run dev

# ── Test ───────────────────────────────────────────────────────────────────────

.PHONY: test
test: install-dev  ## Run full test suite
	$(PYTEST)

.PHONY: test-unit
test-unit: install-dev  ## Run unit tests only
	$(PYTEST) tests/test_*.py -v

.PHONY: test-integration
test-integration: install-dev  ## Run integration tests (in-memory storage)
	$(PYTEST) tests/integration/ -v

.PHONY: test-integration-sqlite
test-integration-sqlite: install-dev  ## Run integration tests against a real SQLite database
	CLOUDTWIN_STORAGE_MODE=sqlite $(PYTEST) tests/integration/ -v

.PHONY: test-verbose
test-verbose: install-dev  ## Run full test suite with verbose output
	$(PYTEST) -v

# ── Quality ────────────────────────────────────────────────────────────────────

.PHONY: lint
lint: install-dev  ## Run ruff linter
	$(VENV)/bin/ruff check src/

.PHONY: format
format: install-dev  ## Auto-format source with ruff
	$(VENV)/bin/ruff format src/

# ── Docker ─────────────────────────────────────────────────────────────────────

DOCKER_IMAGE ?= cloudtwin
DOCKER_TAG   ?= dev

.PHONY: docker-build
docker-build:  ## Build the Docker image (DOCKER_IMAGE=x DOCKER_TAG=y)
	docker build -t $(DOCKER_IMAGE):$(DOCKER_TAG) -f docker/Dockerfile .

.PHONY: docker-run
docker-run:  ## Run the Docker container (ephemeral)
	docker run --rm -p 4793:4793 $(DOCKER_IMAGE):$(DOCKER_TAG)

.PHONY: docker-push
docker-push:  ## Push image to Docker Hub (DOCKER_IMAGE=user/cloudtwin DOCKER_TAG=x)
	docker push $(DOCKER_IMAGE):$(DOCKER_TAG)

# ── Cleanup ────────────────────────────────────────────────────────────────────

.PHONY: clean
clean:  ## Remove venv, caches, and build artifacts
	rm -rf $(VENV) .pytest_cache __pycache__ dist build
	find . -type d -name "__pycache__" -exec rm -rf {} +
	find . -type f -name "*.pyc" -delete

# ── Help ───────────────────────────────────────────────────────────────────────

.PHONY: help
help:  ## Show this help message
	@grep -E '^[a-zA-Z_-]+:.*##' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*##"}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'
