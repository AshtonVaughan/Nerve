# Benchmark methodology

The benchmark harness is intentionally lean. It is not a competitor to
OSWorld, VisualWebArena, or MMINT. Its job is to make local regressions
obvious without requiring a GPU, a vision LLM, or third-party SaaS.

## What we measure

Each benchmark task records ten metrics, defined to be agent-agnostic:

| Metric                       | Definition |
| ---------------------------- | ---------- |
| `task_success`               | Did the run satisfy the task's success oracle? In dry-run mode the oracle inspects the submitted action types (the daemon never touches the OS), so a task passes when it submitted every action without an `ok=false`. Tasks can override with a custom oracle in `benchmarks/harness/tasks.py`. |
| `task_duration_ms`           | Wall-clock from harness start to last result. |
| `model_calls`                | Calls into the model adapter. Zero for the mock agent; non-zero for production adapters. |
| `action_count`               | Total actions submitted. |
| `screenshot_count`           | Actions that exchanged a full screenshot with the daemon. |
| `failed_actions`             | Actions that returned `ok=false`. |
| `recovery_attempts`          | Number of compiler / safety fallbacks recorded for the task. |
| `human_interventions`        | `confirmation_required` events that needed a human. |
| `est_cost_usd`               | Sum of `model_calls × per-call cost`. Zero for mock. |
| `avg_action_latency_ms`      | Average per-action wall-clock from submit to response. |

The first eight come straight from the audit log. `est_cost_usd` is filled
in by the adapter (placeholders return zero). `avg_action_latency_ms`
captures the runtime cost of an action — a key proxy for how snappy a UI
agent feels.

## How we measure

The harness:

1. Optionally spawns the daemon (`--auto-start`) in dry-run mode.
2. Connects via the Python SDK with a `SafetyPolicy(dry_run=True,
   max_actions_per_minute=2400)` so the rate-limit can't influence
   benchmarks.
3. Loops over the static [`TASKS`](../benchmarks/harness/tasks.py),
   running each task's `plan` action by action.
4. Writes one `bench-<timestamp>.json` file per run under
   `benchmarks/results/`.

In live mode (`--live`) the harness disables dry-run. That is the
configuration to use when benchmarking a real workflow on a real desktop.

## Tasks

The shipped tasks are deliberately minimal: open a text editor, save a
file, rename a file, use the calculator, open a browser, search, copy,
paste, fill a local form, change an app setting. They exercise every
major branch of the executor and compiler without depending on the
contents of any web page.

To add a task:

1. Append a `BenchTask` to `TASKS`.
2. Spell out the expected method sequence (in lowering ladder order).
3. Run `python -m benchmarks.harness.runner --auto-start`.

## Why the metrics matter

* `screenshot_count` divided by `action_count` tracks the runtime's
  efficacy at *not* talking to the model. CUA-style loops sit near 1.0;
  Nerve's design target is &lt; 0.3.
* `model_calls / action_count` tracks the same idea from the agent side —
  how often did the agent have to re-decide?
* `avg_action_latency_ms` separates "the model is slow" from "the runtime
  is slow". Pure runtime latency under dry-run mode is the floor; live
  mode adds OS time.
* `recovery_attempts` flags places where the compiler had to fall back —
  if the same task always exercises the OCR fallback, the accessibility
  story for that app is broken.

## Cassette workflow

Running a task end-to-end against a real model adapter (Anthropic CUA,
OpenAI CUA) costs tokens and is non-deterministic. CI needs neither
cost nor flake, so the harness supports a *cassette* mode that replays
a recorded action sequence instead of asking the model live.

Cassettes are plain JSON files under `benchmarks/cassettes/<task>.json`.
The schema:

```json
{
  "task": "clipboard_copy_paste",
  "description": "...",
  "recorded_with": "anthropic|openai|mock|static",
  "recorded_at": 1714670000,
  "actions": [ { "type": "...", ... }, ... ]
}
```

Recording a cassette (requires the relevant API key):

```bash
ANTHROPIC_API_KEY=sk-ant-... \
    python -m benchmarks.harness.record \
        --task calculator_demo --adapter anthropic
```

Replaying every task that has a cassette in CI:

```bash
python -m benchmarks.harness.runner --auto-start --cassettes
```

Tasks without a cassette fall back to their static plan, so the same
command runs the full suite either way. Today three tasks have
cassettes and pass deterministically in CI:

* `clipboard_copy_paste`
* `calculator_demo`
* `change_setting`

Adding more is a matter of (a) writing a task definition with an
`oracle` that's stable across runs and (b) recording the cassette.

## Limitations

The MVP benchmark:

* doesn't measure visual correctness (no OCR / pixel diff yet).
* doesn't drive a real model (mock agent only by default).
* tracks dry-run by default, which means OS interaction errors are
  invisible.

These are intentional gaps for a v0 — they keep the benchmark
deterministic on CI. The real-model harness lives behind a flag for
operators who actually want to measure their agent.
