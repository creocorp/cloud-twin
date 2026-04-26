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

.PHONY: demo
demo: install-dev  ## Seed the running server with demo data (requires: make run-with-dashboard)
	$(PYTHON) scripts/demo.py

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

# ── Rust / cloudtwin-lite ──────────────────────────────────────────────────────

RUST_DIR = rust/cloudtwin-lite

.PHONY: rust-build
rust-build:  ## Build cloudtwin-lite (release)
	cd $(RUST_DIR) && cargo build --release

.PHONY: rust-build-debug
rust-build-debug:  ## Build cloudtwin-lite (debug)
	cd $(RUST_DIR) && cargo build

.PHONY: rust-run
rust-run:  ## Run cloudtwin-lite with in-memory storage
	cd $(RUST_DIR) && \
		CLOUDTWIN_CONFIG_PATH=$(CURDIR)/config/cloudtwin.yml \
		CLOUDTWIN_DB_PATH=:memory: \
		cargo run

.PHONY: rust-run-with-dashboard
rust-run-with-dashboard:  ## Run cloudtwin-lite with the dashboard (http://localhost:4793/dashboard)
	mkdir -p data
	cd $(RUST_DIR) && \
		CLOUDTWIN_CONFIG_PATH=$(CURDIR)/config/cloudtwin.yml \
		CLOUDTWIN_DB_PATH=$(CURDIR)/data/cloudtwin-lite.db \
		CLOUDTWIN_DASHBOARD_STATIC=$(CURDIR)/dashboard/static \
		cargo run

.PHONY: rust-test
rust-test:  ## Run cloudtwin-lite unit/integration tests
	cd $(RUST_DIR) && cargo test

.PHONY: rust-test-parity
rust-test-parity:  ## Run the Bedrock integration suite against a running cloudtwin-lite binary (build first with `make rust-build`)
	@echo "Starting cloudtwin-lite on :47930 with in-memory storage..."
	@CLOUDTWIN_CONFIG_PATH=$(CURDIR)/config/cloudtwin.yml \
		CLOUDTWIN_PORT=47930 \
		CLOUDTWIN_DB_PATH=:memory: \
		$(RUST_DIR)/target/release/cloudtwin-lite & \
		echo $$! > /tmp/cloudtwin-lite-parity.pid; \
		sleep 1; \
		CLOUDTWIN_TEST_URL=http://127.0.0.1:47930 $(PYTEST) tests/integration/providers/aws/test_bedrock_boto3.py -v; \
		status=$$?; \
		kill $$(cat /tmp/cloudtwin-lite-parity.pid) 2>/dev/null; \
		rm -f /tmp/cloudtwin-lite-parity.pid; \
		exit $$status

.PHONY: rust-check
rust-check:  ## Check cloudtwin-lite compiles without producing binaries
	cd $(RUST_DIR) && cargo check

.PHONY: rust-clippy
rust-clippy:  ## Run clippy linter on cloudtwin-lite
	cd $(RUST_DIR) && cargo clippy -- -D warnings

.PHONY: rust-clean
rust-clean:  ## Remove cloudtwin-lite build artifacts
	cd $(RUST_DIR) && cargo clean

.PHONY: docker-build-lite
docker-build-lite:  ## Build the cloudtwin-lite Docker image
	docker build -t cloudtwin-lite:dev -f docker/Dockerfile.lite .

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
