#!/usr/bin/env bash
# Run the suite inside Linux.
#
# Everything here runs against real processes: a real shell on a real pty, a
# real socket, a real git repository. The point is to catch what only exists
# when it all runs at once — which is where every bug in this project so far
# has actually been.
#
# Linux specifically, because the two platform bugs this project has had —
# a blocking pty read and an unreadable /proc — both showed up on one platform
# and not the other.
set -euo pipefail

cd "$(dirname "$0")/../.."

echo "==> building the fixture"
docker build -q -t omt-e2e -f tests/e2e/Dockerfile tests/e2e >/dev/null

echo "==> running in Linux"
docker run --rm \
  -v "$PWD":/omt \
  -w /omt \
  omt-e2e \
  bash -euo pipefail -c '
    fail=0

    echo "--- workspace ---"
    if ! cargo test --workspace --quiet > /tmp/workspace.log 2>&1; then
      fail=1
    fi
    # Summarised rather than tailed: a tail of a thousand lines shows the last
    # four test binaries and hides the one that failed.
    grep -E "^(test result|error)" /tmp/workspace.log \
      | awk -F"[ ;]" "/test result: ok/{ok+=\$4} /FAILED/{bad++} END{
          printf \"  %d passed, %d suites failed\n\", ok, bad+0 }"
    grep -B5 "test result: FAILED" /tmp/workspace.log | head -40 || true

    echo "--- end to end ---"
    if ! cargo test --quiet --test e2e -p omt -- --test-threads=1 > /tmp/e2e.log 2>&1; then
      fail=1
    fi
    grep -E "^(test |test result)" /tmp/e2e.log | tail -20

    exit "$fail"
  '
