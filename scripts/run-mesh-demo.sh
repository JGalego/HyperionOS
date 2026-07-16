#!/usr/bin/env bash
# Real, many-instance Hyperion "Social" mesh demo (docs/998-roadmap.md's Social pillar, docs/
# 999-usage-scenarios.md's mesh scenario). Launches several real, separately-started
# `hyperion-console` processes on this same LAN, each advertising a different capability over
# real mDNS and dispatching through a real cloud LLM (Anthropic/OpenAI/Groq -- see the `NODES`
# table below), several of which then really delegate to whichever peer actually has what they
# lack (see `crates/hyperion-console/src/mesh.rs`) -- plus one more real instance running
# `/mesh-dashboard`, a live browser page that watches the whole mesh discover and delegate,
# colored by which real backend each node is actually using.
#
# No human is in the delegation loop: every "who has this?" / "ask them" decision below is a real
# node making its own real request to another real node.
#
# Needs real API keys for whichever providers `NODES` names below, in `.env` at the repo root
# (never hardcoded here, never printed -- see the redacted "[key redacted]" echo this crate's own
# `run_scenario_file` already gives a pasted secret): `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`,
# `GROQ_API_KEY`.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BIN="$REPO_ROOT/target/debug/hyperion-console"
DEMO_DIR="$(mktemp -d -t hyperion-mesh-demo-XXXXXX)"
DASHBOARD_PORT=8767

if [[ -f "$REPO_ROOT/.env" ]]; then
    set -a
    # shellcheck source=/dev/null
    source "$REPO_ROOT/.env"
    set +a
fi

echo "Building hyperion-console (--features mdns,openai-compat,anthropic)…"
cargo build -p hyperion-console --bin hyperion-console --features mdns,openai-compat,anthropic \
    --manifest-path "$REPO_ROOT/Cargo.toml"

# Every node ends its own scenario with `/standby`, which blocks on a real read of its own stdin
# until this script is done with it -- inheriting *this* script's own stdin verbatim would tie
# that block to whatever stdin this script itself happened to be launched with (a real terminal,
# a pipe, a closed/non-interactive fd under some CI or sandboxed invocation), which isn't this
# script's own to depend on. A real FIFO this script opens read-write itself (so no reader ever
# blocks on `open()` waiting for a writer that doesn't exist) gives every node a real, stable
# stdin that only ever ends when this script itself does.
CONTROL_FIFO="$DEMO_DIR/control.fifo"
mkfifo "$CONTROL_FIFO"
exec 9<>"$CONTROL_FIFO"

PIDS=()
trap 'echo; echo "Stopping the mesh ($DEMO_DIR)…"; kill "${PIDS[@]}" 2>/dev/null || true' EXIT

# Blocks until a real socket on $1 actually accepts a connection -- a fixed, bounded poll-connect
# (not a guessed sleep), the same discipline this crate's own integration tests use.
wait_for_port() {
    local port="$1" deadline=$((SECONDS + 15))
    until (echo > "/dev/tcp/127.0.0.1/$port") 2>/dev/null; do
        if (( SECONDS >= deadline )); then
            echo "node on port $port never came up -- check $DEMO_DIR/*/output.log" >&2
            exit 1
        fi
        sleep 0.2
    done
}

# name:port:capabilities:requests:provider:model -- "requests" is a `;`-separated list of
# `capability|message text` pairs this node itself delegates, right after it starts serving its
# own; "provider"/"model" pick this node's real cloud backend (see `connect my <provider>
# account` + `/backend <provider> <model>` in the generated scenario below) -- empty means stay
# on the deterministic `MockBackend`. Deliberately overlapping/staggered: kenji/priya serve one
# capability each; sam/dana both serve *and* request (from kenji, already running by the time
# they start); milo/aria are pure requesters, started last so every capability they ask for
# already has a real, discoverable provider. Two nodes per real provider -- enough for the
# dashboard's own backend-color legend to mean something without a color per node.
# Request text carries its own real content -- no attached greeting/paragraph/photo exists over
# this text-only protocol, and a real model correctly (if unhelpfully, for a demo) says so if the
# request implies one it was never given. `image-edit` in particular can't be exercised as "edit
# this photo" with nothing attached; phrased as a self-contained, answerable question about
# editing a described photo instead, a real model can actually complete it.
NODES=(
    "kenji:9101:translate-ja::anthropic:claude-haiku-4-5-20251001"
    "priya:9102:image-edit::openai:gpt-4o-mini"
    "sam:9103:summarize:translate-ja|please translate this greeting to Japanese - Good morning, I hope you have a wonderful day:groq:llama-3.1-8b-instant"
    "dana:9104:image-edit,summarize:translate-ja|translate this label to Japanese - Emergency Exit:anthropic:claude-haiku-4-5-20251001"
    "milo:9105:hyperion.ask:summarize|please summarize this paragraph - Hyperion is an intent-native operating system. Instead of asking users to open specific apps or manage files directly, it lets people state a goal in plain language, then figures out which capability, tool, or agent should carry it out. The system is built to be resourceful, self-sustaining, and social, treating other real Hyperion instances as peers to delegate to, not silos.;image-edit|I have a photo I want to crop to a square for Instagram without cutting off a centered subject - what crop dimensions should I use:openai:gpt-4o-mini"
    "aria:9106:hyperion.ask:translate-ja|say hello in Japanese;image-edit|I need to resize a 1920 by 1080 photo down to 600 pixels wide for a website thumbnail while keeping the aspect ratio - what height should I use:groq:llama-3.1-8b-instant"
)

for spec in "${NODES[@]}"; do
    IFS=':' read -r name port caps requests provider model <<< "$spec"
    node_dir="$DEMO_DIR/$name"
    mkdir -p "$node_dir"
    scenario="$node_dir/scenario.txt"

    {
        echo "/a2a-server $port $name"
        if [[ -n "$provider" ]]; then
            # The API key line is `$<PROVIDER>_API_KEY` (a literal, escaped `$`) -- expanded
            # against this real process's own environment by `hyperion-console`'s own
            # `expand_env_vars`, never by this script, so the real secret is never written to
            # this scenario file on disk.
            echo "connect my $provider account"
            echo "\$${provider^^}_API_KEY"
            echo "/backend $provider $model"
        fi
        if [[ -n "$requests" ]]; then
            IFS=';' read -ra reqs <<< "$requests"
            for req in "${reqs[@]}"; do
                echo "/mesh-request $port ${req%%|*} ${req#*|}"
            done
        fi
        echo "/standby"
    } > "$scenario"

    echo "Starting $name on port $port (capabilities: $caps; backend: ${provider:-mock}${model:+ $model})…"
    HYPERION_CONSOLE_DATA_DIR="$node_dir/data" HYPERION_CONSOLE_CAPABILITIES="$caps" \
        "$BIN" "$scenario" < "$CONTROL_FIFO" > "$node_dir/output.log" 2>&1 &
    PIDS+=("$!")
    wait_for_port "$port"
    # A real, bound TCP port doesn't mean this node's own mDNS advertisement has actually
    # propagated yet (that's a separate background daemon) -- a short real pause here gives it
    # time to settle before the next node starts asking around.
    sleep 1
done

dash_dir="$DEMO_DIR/dashboard"
mkdir -p "$dash_dir"
printf '/mesh-dashboard %s\n/standby\n' "$DASHBOARD_PORT" > "$dash_dir/scenario.txt"
HYPERION_CONSOLE_DATA_DIR="$dash_dir/data" \
    "$BIN" "$dash_dir/scenario.txt" < "$CONTROL_FIFO" > "$dash_dir/output.log" 2>&1 &
PIDS+=("$!")
wait_for_port "$DASHBOARD_PORT"

echo
echo "Mesh dashboard:  http://127.0.0.1:$DASHBOARD_PORT"
echo "Per-node logs:   $DEMO_DIR/<name>/output.log"
echo "Press Ctrl-C here to stop every real node."
wait
