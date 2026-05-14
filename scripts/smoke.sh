#!/usr/bin/env bash
# End-to-end smoke test for the Nerve daemon.
#
# 1. Build the release binary.
# 2. Boot it under --dry-run.
# 3. Hit each CLI subcommand the dashboard depends on.
# 4. Drive the Python demo + benchmark harness.
# 5. Tear down the daemon.

set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

BIN="core/target/release/nerve"
LOG="/tmp/nerve-smoke.log"
PORT="${NERVE_PORT:-8765}"
FEATURES="${NERVE_FEATURES:-ocr-tesseract}"

if [ "${NERVE_REBUILD:-0}" = "1" ] || [ ! -x "$BIN" ]; then
  (cd core && cargo build --release --features "$FEATURES")
fi

# Boot daemon
RUST_LOG=info "$BIN" start --dry-run > "$LOG" 2>&1 &
DAEMON_PID=$!
trap 'kill -9 $DAEMON_PID 2>/dev/null || true' EXIT

# Wait for bind
for _ in $(seq 1 100); do
  if (echo > /dev/tcp/127.0.0.1/$PORT) >/dev/null 2>&1; then
    break
  fi
  sleep 0.1
done

echo "==> nerve status"
"$BIN" status

echo "==> nerve capabilities (truncated)"
"$BIN" capabilities | head -16
echo "==> assert: ocr capability matches build features"
OCR_FLAG=$("$BIN" capabilities | python3 -c "import json,sys; print(json.load(sys.stdin)['ocr'])")
case ",$FEATURES," in
  *,ocr-tesseract,*)
    if [ "$OCR_FLAG" != "True" ]; then
      echo "FAIL: built with ocr-tesseract but capabilities.ocr=$OCR_FLAG"
      exit 1
    fi
    ;;
  *)
    if [ "$OCR_FLAG" != "False" ]; then
      echo "FAIL: built without ocr-tesseract but capabilities.ocr=$OCR_FLAG"
      exit 1
    fi
    ;;
esac
echo "ocr capability ($OCR_FLAG) matches features ($FEATURES)"

echo "==> python demo"
PYTHONPATH=sdks/python python3 -m agents.demo.run_demo | tail -20

echo "==> benchmark harness"
PYTHONPATH=sdks/python python3 -m benchmarks.harness.runner | tail -15

echo "==> /metrics (truncated)"
curl -s http://127.0.0.1:$PORT/metrics | grep -E "^nerve_" | head -10

echo "==> done; daemon log:"
tail -10 "$LOG"
