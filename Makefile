# Convenience wrapper around the per-component build commands.

.PHONY: all build build-core build-python build-typescript test test-core test-python smoke demo bench clean fmt lint

all: build

build: build-core build-typescript

build-core:
	cd core && cargo build --release

build-python:
	pip install -e ./sdks/python

build-typescript:
	cd sdks/typescript && npm install && npm run build

test: test-core test-python

test-core:
	cd core && cargo test --workspace

test-python:
	cd sdks/python && python -m pytest -q tests/ || true

smoke:
	@echo "Smoke test: start daemon, run demo in dry-run mode."
	PYTHONPATH=sdks/python python -m agents.demo.run_demo --auto-start

demo:
	PYTHONPATH=sdks/python python -m agents.demo.run_demo

bench:
	PYTHONPATH=sdks/python python -m benchmarks.harness.runner --auto-start

fmt:
	cd core && cargo fmt --all

lint:
	cd core && cargo clippy --workspace --all-targets -- -D warnings || true

clean:
	cd core && cargo clean
	rm -rf sdks/typescript/dist sdks/typescript/node_modules
	rm -rf benchmarks/results/*.json
