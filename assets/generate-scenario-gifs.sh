#!/usr/bin/env bash
# Regenerates assets/demo-onboarding.gif, assets/demo-knowledge-graph.gif, and
# assets/demo-cloud-providers.gif -- each a real, live VHS recording of the matching scenario
# file under scenarios/, run against the real, compiled hyperion-console binary. Nothing here is
# staged or hand-edited output; see each assets/vhs/*.tape file's own header for what it actually
# records and why.
#
# Must run from the repo root (VHS's own internal shell -- and therefore every relative path a
# .tape file's Type command references, like "scenarios/..." or "./target/debug/..." -- inherits
# whatever directory `vhs` itself was invoked from, not the .tape file's own location).
#
# Requires:
#   - vhs (https://github.com/charmbracelet/vhs):  go install github.com/charmbracelet/vhs@latest
#   - ttyd (https://github.com/tsl0922/ttyd), a headless Chromium, and ffmpeg -- vhs's own real
#     runtime dependencies, used to drive and capture the recording
#   - hyperion-console built with `--features openai-compat,anthropic` (assets/vhs/cloud-providers.tape
#     needs it; the other two work against a plain default build too)
#   - a real, valid OPENAI_API_KEY, ANTHROPIC_API_KEY, and GROQ_API_KEY in .env, only for
#     assets/vhs/cloud-providers.tape -- regenerating that one costs three real paid API calls.

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

if ! command -v vhs >/dev/null; then
  echo "vhs isn't on \$PATH -- see this script's own header for how to install it." >&2
  exit 1
fi

echo "Recording assets/demo-onboarding.gif..."
vhs assets/vhs/onboarding.tape

echo "Recording assets/demo-knowledge-graph.gif..."
vhs assets/vhs/knowledge-graph.tape

echo "Recording assets/demo-cloud-providers.gif (real paid API calls)..."
vhs assets/vhs/cloud-providers.tape

echo "Done."
