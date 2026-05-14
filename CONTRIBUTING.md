# Contributing to Nerve

We accept patches, issue reports, and adapters for new models. Please read
the safety docs ([`docs/safety.md`](docs/safety.md),
[`docs/threat-model.md`](docs/threat-model.md)) before touching the
execution path.

## Setup

```bash
git clone https://github.com/ashtonvaughan/nerve.git
cd nerve

# Rust core
(cd core && cargo build)

# Python SDK + tests
pip install -e ./sdks/python pytest
pytest sdks/python/tests

# TypeScript SDK + tests
(cd sdks/typescript && npm install && npm run build && npm test)
```

## Branch + commit conventions

* Develop on a branch off `main`.
* Use imperative subject lines (`Add foo`, `Fix bar`).
* Mention the affected crate / sdk in the first line, e.g.
  `nerve-core: bound the idempotency cache`.

## Tests

* Rust unit tests: `cargo test --workspace --lib`.
* Rust integration tests: `cargo test --workspace --test integration`.
* Python: `pytest sdks/python/tests`.
* TypeScript: `npm test` in `sdks/typescript/`.

CI runs the matrix on every push (Ubuntu / macOS / Windows). PRs cannot
land if any matrix entry is red.

## Adding a new model adapter

1. Drop a new module under `agents/<name>/` mirroring the layout of
   `agents/openai/__init__.py`.
2. Implement the `plan(observation, state)` method on a class with a stable
   `name` attribute.
3. Add at least one fixture-driven test under `agents/<name>/tests/` so the
   parser stays honest.
4. Register the adapter in `docs/quickstart.md` and `agents/README.md`.

## Adding a new platform backend

1. Add the per-OS module behind a Cargo feature flag (mirror
   `platform/macos_native.rs`).
2. Update `platform/<os>.rs` to call into the new code when the feature is
   enabled.
3. Make sure the default build (feature off) still compiles cleanly on every
   OS in the CI matrix.
4. Document any new permission grants the user must perform.

## Style

* Format Rust with `cargo fmt`. CI runs `--check`.
* Lint with `cargo clippy --workspace --all-targets -- -D warnings`.
* Python: standard library only unless a feature genuinely needs a dep.
* TypeScript: `tsc` strict mode.

## Discussion

GitHub Discussions for ideas / requests for comment. Issues for tracked
work. Pull requests for landing code.
