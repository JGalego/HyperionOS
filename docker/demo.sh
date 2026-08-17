#!/usr/bin/env bash
# A real, many-instance Hyperion mesh, in containers, on your machine.
#
# Three separate Hyperion processes wake up, find each other over real mDNS, and one of them
# delegates a goal it cannot do itself to a peer that can -- over the real A2A transport, with a
# real trust-on-first-use record. Then a peer disappears mid-conversation and comes back, so you
# can watch the mesh degrade and recover instead of being told that it does.
#
# Nothing here is staged: every line of output below comes from a real Hyperion console reacting
# to a real message from a real peer. The only thing mocked is the model itself (the built-in mock
# backend), so the demo needs no API key and no network.
#
#   ./docker/demo.sh            # the guided demo
#   ./docker/demo.sh --fast     # same, without the dramatic pauses
#
# See docker/README.md for what this does and does not prove -- in particular, why the nodes share
# one network namespace rather than sitting on separate container IPs.
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="${HYPERION_IMAGE:-hyperion-node:dev}"
ANCHOR="hyperion-mesh-net"   # full container name; nodes join its netns
RUNTIME="$(mktemp -d)"
FAST=""
[[ "${1:-}" == "--fast" ]] && FAST=1

# Node identity: name, A2A port, and the capabilities its Agent Card advertises to the mesh.
ATLAS_PORT=8600
BEACON_PORT=8601
CINDER_PORT=8602
DASHBOARD_PORT=8801        # where the dashboard really binds, on the shared loopback
BROWSER_PORT=8800          # what the forwarder publishes to your machine -- see the anchor below

bold=$'\033[1m'; dim=$'\033[2m'; reset=$'\033[0m'
gold=$'\033[38;2;217;165;74m'; green=$'\033[38;2;143;174;106m'
blue=$'\033[38;2;122;162;198m'; red=$'\033[38;2;198;120;110m'

say()   { printf '\n%s%s%s\n' "$bold" "$1" "$reset"; }
note()  { printf '%s%s%s\n' "$dim" "$1" "$reset"; }
beat()  { [[ -n "$FAST" ]] || sleep "${1:-2}"; }

cleanup() {
    printf '\n%s' "$dim"
    for n in atlas beacon cinder; do docker rm -f "hyperion-$n" >/dev/null 2>&1; done
    docker rm -f "$ANCHOR" >/dev/null 2>&1
    rm -rf "$RUNTIME"
    printf 'mesh torn down.%s\n' "$reset"
}
trap cleanup EXIT INT TERM

require_image() {
    if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
        say "Building the node image first (one time, ~1 minute)..."
        "$REPO_ROOT/docker/build.sh" || exit 1
    fi
}

# Every node writes to its own fifo; holding the write end open is what keeps the console alive,
# since it exits at end-of-input like any other well-behaved stdin-driven program.
start_node() {
    local name="$1" port="$2" capabilities="$3"
    rm -f "$RUNTIME/$name.in"
    mkfifo "$RUNTIME/$name.in"
    mkdir -p "$RUNTIME/$name.data"
    docker run -i --rm --name "hyperion-$name" \
        --network "container:$ANCHOR" \
        -v "$RUNTIME/$name.data:/var/lib/hyperion" \
        -e "HYPERION_CONSOLE_CAPABILITIES=$capabilities" \
        "$IMAGE" < "$RUNTIME/$name.in" > "$RUNTIME/$name.out" 2>&1 &
    sleep 0.4
}

send() { printf '%s\n' "$2" >&"$1"; }

# Shows only what a node printed since the last time we looked, so each beat reads as that node
# reacting now rather than a growing wall of scrollback.
show() {
    local name="$1" colour="$2" label="$3"
    local marker="$RUNTIME/$name.seen"
    local seen=0
    [[ -f "$marker" ]] && seen="$(cat "$marker")"
    local total; total="$(wc -l < "$RUNTIME/$name.out")"
    if (( total > seen )); then
        printf '%s  %s%s\n' "$colour" "$label" "$reset"
        tail -n +$((seen + 1)) "$RUNTIME/$name.out" \
            | grep -vE '^\s*$' | grep -v '^You ask. I understand.$' \
            | sed 's/^/    /'
    fi
    echo "$total" > "$marker"
}

require_image

clear 2>/dev/null || true
cat <<BANNER
${gold}
   _  ___   _____ ___ ___ ___ ___  _  _
  | || \\ \\ / / _ \\ __| _ \\_ _/ _ \\| \\| |
  | __ |\\ V /|  _/ _||   /| | (_) | .\` |
  |_||_| |_| |_| |___|_|_\\___\\___/|_|\\_|${reset}

  ${bold}Three machines. One goal. No one in charge.${reset}
BANNER

say "1 / 5   Three Hyperions wake up"
note "Each is a separate process in its own container, ${IMAGE} -- a 14 MB image built FROM scratch:"
note "one static binary and a CA bundle, no distro underneath it."
# The anchor owns the network namespace every node joins, and does one other job. Hyperion's
# servers bind 127.0.0.1 on purpose -- they answer without authentication, so loopback *is* the
# access control (see http_server.rs's `reject_reason`). Docker's published ports forward to a
# container's external interface, not its loopback, so a browser on your machine cannot reach the
# dashboard directly. socat bridges that one port explicitly, which is the honest way to do it:
# the forwarding is visible here in the demo harness rather than hidden in a weakened bind address.
docker rm -f "$ANCHOR" >/dev/null 2>&1
docker run -d --rm --name "$ANCHOR" -p "$BROWSER_PORT:$BROWSER_PORT" \
    alpine/socat "TCP-LISTEN:$BROWSER_PORT,fork,reuseaddr" "TCP:127.0.0.1:$DASHBOARD_PORT" \
    >/dev/null
beat 1

start_node atlas  "$ATLAS_PORT"  "translate-ja"
start_node beacon "$BEACON_PORT" "market-research,summarize"
start_node cinder "$CINDER_PORT" "hyperion.ask"
exec 3> "$RUNTIME/atlas.in"; exec 4> "$RUNTIME/beacon.in"; exec 5> "$RUNTIME/cinder.in"
sleep 2

note ""
note "  atlas   speaks Japanese          (translate-ja)"
note "  beacon  researches and summarizes (market-research, summarize)"
note "  cinder  knows only how to ask     (hyperion.ask)"
beat 2

say "2 / 5   They announce themselves, and start listening for each other"
send 3 "/a2a-server $ATLAS_PORT atlas"
send 4 "/a2a-server $BEACON_PORT beacon"
send 5 "/a2a-server $CINDER_PORT cinder"
sleep 4
show atlas  "$gold"  "atlas"
show beacon "$blue"  "beacon"
show cinder "$green" "cinder"
note ""
note "Real mDNS/DNS-SD advertisements on _hyperion-a2a._tcp.local. Nobody was given a peer list."
beat 3

say "3 / 5   cinder is asked for something it cannot do"
note "cinder has no translator. It does not fail, and it does not guess -- it looks for someone."
send 5 "/mesh-request $CINDER_PORT translate-ja Good morning, thank you for your work"
sleep 12
show cinder "$green" "cinder"
show atlas  "$gold"  "atlas"
note ""
note "cinder discovered atlas by capability, not by address, read its Agent Card to confirm it"
note "really advertises translate-ja, delegated over A2A, and recorded atlas's public identity."
note "That last part matters: the trust is pinned now, so an impostor answering later is caught."
beat 3

say "4 / 5   atlas disappears mid-conversation"
docker rm -f hyperion-atlas >/dev/null 2>&1
printf '%s  atlas is gone.%s\n' "$red" "$reset"
beat 2
send 5 "/mesh-request $CINDER_PORT translate-ja Please try again"
sleep 14
show cinder "$green" "cinder"
note ""
note "No crash, no hang, no invented translation -- it says plainly that nobody answered."
beat 3

say "5 / 5   atlas comes back"
exec 3>&-
start_node atlas "$ATLAS_PORT" "translate-ja"
exec 3> "$RUNTIME/atlas.in"
sleep 2
send 3 "/a2a-server $ATLAS_PORT atlas"
sleep 4
send 5 "/mesh-request $CINDER_PORT translate-ja Good morning, thank you for your work"
sleep 12
show cinder "$green" "cinder"
note ""
note "Delegation works again -- and notice what is *missing*: no \"trusting for the first time\""
note "line. cinder remembered atlas's identity across the restart and checked it silently."
beat 2

say "The mesh, live"
send 5 "/mesh-dashboard $DASHBOARD_PORT"
sleep 3
show cinder "$green" "cinder"
printf '\n  %sOpen%s  http://localhost:%s  %s-- refreshes itself every couple of seconds%s\n' \
    "$bold" "$reset" "$BROWSER_PORT" "$dim" "$reset"
printf '\n  %sPress Enter to tear the mesh down.%s\n' "$dim" "$reset"
read -r _ || true
