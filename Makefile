VENV   := .venv
PYTHON := $(VENV)/bin/python
PIP    := $(VENV)/bin/pip
PYTEST := $(VENV)/bin/pytest

.DEFAULT_GOAL := help

# ── Build & run ────────────────────────────────────────────────────────────────

.PHONY: build
build:  ## Build the release binary (target/release/cloudtwin)
	cargo build --release

.PHONY: build-debug
build-debug:  ## Build the debug binary (target/debug/cloudtwin)
	cargo build

.PHONY: check
check:  ## Type-check without producing binaries
	cargo check

.PHONY: run
run:  ## Run CloudTwin with in-memory storage (port 4793)
	CLOUDTWIN_CONFIG_PATH=$(CURDIR)/config/cloudtwin.yml \
		CLOUDTWIN_DB_PATH=:memory: \
		cargo run

.PHONY: run-with-dashboard
run-with-dashboard:  ## Run with SQLite in data/ and the dashboard (http://localhost:4793/dashboard)
	mkdir -p data
	CLOUDTWIN_CONFIG_PATH=$(CURDIR)/config/cloudtwin.yml \
		CLOUDTWIN_DB_PATH=$(CURDIR)/data/cloudtwin.db \
		CLOUDTWIN_DASHBOARD_STATIC=$(CURDIR)/dashboard/static \
		cargo run

# ── Test & lint ────────────────────────────────────────────────────────────────

$(VENV)/bin/activate:
	python3 -m venv $(VENV)
	$(PIP) install --upgrade pip

.PHONY: install-test
install-test: $(VENV)/bin/activate  ## Install the Python SDK test dependencies into .venv
	$(PIP) install -r tests/requirements.txt

.PHONY: test
test: test-unit test-integration  ## Run unit tests and SDK integration tests

.PHONY: test-unit
test-unit:  ## Run in-process Rust tests (cargo test)
	cargo test

.PHONY: test-integration
test-integration: build-debug install-test  ## Run the boto3/Azure/GCP SDK suites against target/debug/cloudtwin
	$(PYTEST) tests -v

.PHONY: test-integration-sqlite
test-integration-sqlite: build-debug install-test  ## Same, but against a file-backed SQLite database
	CLOUDTWIN_STORAGE_MODE=sqlite $(PYTEST) tests -v

.PHONY: lint
lint:  ## cargo fmt --check + clippy (-D warnings)
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings

.PHONY: format
format:  ## Format the source with rustfmt
	cargo fmt

.PHONY: demo
demo: install-test  ## Seed a running server with demo data (requires: make run-with-dashboard)
	$(PYTHON) scripts/demo.py

# ── Dashboard ──────────────────────────────────────────────────────────────────

.PHONY: dashboard-dev
dashboard-dev:  ## Start the Vite dashboard dev server (proxies /api/* to port 4793)
	cd dashboard && npm run dev

# ── Docker ─────────────────────────────────────────────────────────────────────

DOCKER_IMAGE ?= cloudtwin
DOCKER_TAG   ?= dev

.PHONY: docker-build
docker-build:  ## Build the Docker image (DOCKER_IMAGE=x DOCKER_TAG=y)
	docker build -t $(DOCKER_IMAGE):$(DOCKER_TAG) -f docker/Dockerfile .

.PHONY: docker-run
docker-run:  ## Run the Docker container (ephemeral)
	docker run --rm -p 4793:4793 -p 1025:1025 $(DOCKER_IMAGE):$(DOCKER_TAG)

.PHONY: docker-push
docker-push:  ## Push image to Docker Hub (DOCKER_IMAGE=user/cloudtwin DOCKER_TAG=x)
	docker push $(DOCKER_IMAGE):$(DOCKER_TAG)

# ── Cleanup ────────────────────────────────────────────────────────────────────

.PHONY: clean
clean:  ## Remove build artifacts, venv, and caches
	cargo clean
	rm -rf $(VENV) .pytest_cache
	find . -type d -name "__pycache__" -not -path "./target/*" -exec rm -rf {} +

# ── Help ───────────────────────────────────────────────────────────────────────

.PHONY: help
help:  ## Show this help message
	@grep -E '^[a-zA-Z_-]+:.*##' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*##"}; {printf "  \033[36m%-24s\033[0m %s\n", $$1, $$2}'
