# Nerve benchmarks

A small harness that runs canonical tasks against the Nerve runtime and
records the metrics that matter for computer-use agents:

| Metric | Why it matters |
|--------|----------------|
| `task_success` | did the agent complete the task |
| `task_duration_ms` | wall-clock end-to-end |
| `model_calls` | how often the model was woken up |
| `action_count` | how many actions Nerve executed |
| `screenshot_count` | how many full screenshots changed hands |
| `failed_actions` | actions that returned `ok=false` |
| `recovery_attempts` | safety-confirmation or compiler-fallback events |
| `human_interventions` | confirmation events that needed a human |
| `est_cost_usd` | rough cost estimate using per-token prices |
| `avg_action_latency_ms` | how snappy the runtime feels |

The harness is intentionally minimal — it doesn't pretend to replace MMINT,
OSWorld, or VisualWebArena. Its job is to make local regressions obvious
without needing a GPU or third-party agent SaaS.

Run it (with the daemon already running, or use `--auto-start`):

```bash
python -m benchmarks.harness.runner --auto-start
```

Results land in `benchmarks/results/<timestamp>.json`.
