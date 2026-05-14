# Local benchmark tasks

The tasks are defined declaratively in
[`benchmarks/harness/tasks.py`](../harness/tasks.py). They are kept short so
they can run in dry-run mode on CI machines that don't grant input/automation
permission.

To add a task:

1. Append a new `BenchTask` entry to `TASKS`.
2. List the methods you expect Nerve's executor to use, in order.
3. Run the harness — `python -m benchmarks.harness.runner --auto-start`.
