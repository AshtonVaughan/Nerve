# Nerve agents

This directory holds model adapters and example agents.

Adapters wire a particular AI model to Nerve. Each adapter has the same
interface: take an [`Observation`](../core/crates/nerve-protocol/src/observation.rs)
and return a list of `AnyAction`s for Nerve to execute. The model is the brain;
Nerve is the body.

| Folder       | Purpose |
| ------------ | ------- |
| `mock/`      | Deterministic rule-based agent used for tests and the demo |
| `openai/`    | Placeholder adapter for OpenAI Computer Use (CUA) |
| `anthropic/` | Placeholder adapter for Claude Computer Use |
| `gemini/`    | Placeholder for Gemini |
| `ollama/`    | Placeholder for local models via Ollama |
| `vllm/`      | Placeholder for self-hosted vLLM |
| `demo/`      | End-to-end Python demo that exercises the runtime |

Adapters intentionally ship without baked-in API keys. They live behind a
common protocol (`agents/base.py`) so they can be swapped without changes to
the runtime.
