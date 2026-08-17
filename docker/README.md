# Hyperion in containers

A three-node Hyperion mesh on your machine, in about a minute.

```sh
docker/build.sh      # once: cross-compiles the binary, builds a 14 MB image
docker/demo.sh       # the guided demo
```

Three separate Hyperion processes wake up, find each other over real mDNS, and one delegates a
goal it cannot do itself to a peer that can — over the real A2A transport, with a real
trust-on-first-use record. Then a peer disappears mid-conversation and comes back, so you can
watch the mesh degrade and recover rather than be told that it does.

Nothing is staged. Every line the demo prints comes from a real Hyperion console reacting to a
real message from a real peer. The only mocked thing is the model itself — the built-in mock
backend — so it needs no API key and no network.

## The image

`FROM scratch`. Two files: a statically linked `hyperion-console` and a CA bundle. No distro, no
shell, no package manager, and so no distro CVEs to track. ~14 MB.

`hyperion-console` already cross-compiles to static musl for the boot image, so this reuses that
target rather than adding a Rust builder stage and a couple of gigabytes of toolchain cache.

Built with real HTTP/TLS, the OpenAI/Anthropic/Gemini backends, and mDNS. Point a node at a local
model with `/backend ollama http://localhost:11434/v1 llama3`, or at a cloud provider with
`connect my openai account`.

## Driving it yourself

```sh
docker compose -f docker/compose.yml up -d
docker attach hyperion-cinder     # a real Hyperion prompt
```

```
/a2a-server 8602 cinder                        announce yourself to the mesh
/mesh-request 8602 translate-ja Good morning    ask the mesh for something you can't do
/mesh-dashboard 8801                            then open http://localhost:8800
```

Detach with **Ctrl-P Ctrl-Q**. Ctrl-C would stop the container — it forwards to the console.

The three nodes deliberately differ, because a mesh where everyone can do everything proves
nothing:

| node | capabilities |
|---|---|
| `atlas` | `translate-ja` |
| `beacon` | `market-research`, `summarize` |
| `cinder` | `hyperion.ask` — it can only ask |

Set `HYPERION_CONSOLE_CAPABILITIES` to change them.

## Why the nodes share one network namespace

This is the one thing about the setup that isn't obvious, and it's worth explaining rather than
hiding, because it reflects something true about Hyperion today.

Hyperion's A2A, MCP, and dashboard servers **bind `127.0.0.1` on purpose**. They answer without
authentication and every request drives a real agent turn, so loopback *is* the access control —
`http_server.rs`'s `reject_reason` is built on that assumption and says so.

So the nodes join a single network namespace (`--network container:…`, owned by the `mesh-net`
anchor) and meet each other on a shared loopback at different ports. Each is still a wholly
separate process with its own memory, its own knowledge graph, and its own peer-trust file — they
share a network stack, not a brain. That's what "many instances on one machine" honestly looks
like given a loopback-only bind, and it exercises the real discovery, Agent Card, delegation,
and trust code paths end to end.

The anchor also runs a small `socat` forwarder for the dashboard port, because Docker publishes to
a container's external interface rather than its loopback. The forwarding is visible in the demo
harness rather than buried in a weakened bind address.

## What this does not prove

**Cross-machine federation does not work today, and this demo does not show otherwise.**

A node advertises itself over mDNS as reachable on the LAN, but the port it advertises listens
only on loopback. Verified directly:

```
== from inside the node's own netns (127.0.0.1:8600) ==
{"capabilities":{...},"id":"hyperion...          # the Agent Card

== from a different host on the same network (172.18.0.2:8600) ==
wget: can't connect to remote host: Connection refused
```

So the Social pillar works between processes on one machine. Two laptops on the same Wi-Fi would
discover each other and then fail to connect.

This is not a one-line fix, which is why the demo works around it rather than papering over it:
binding `0.0.0.0` would expose an unauthenticated endpoint that runs agent turns to everything on
the network. The pieces for doing it properly already exist — nodes have Ed25519 identities and
already pin a peer's key on first delegation — but that identity is not yet checked on *inbound*
requests. Until it is, loopback is the honest boundary.

## Cleaning up

`docker/demo.sh` tears its own mesh down on exit, including Ctrl-C. For the compose version:

```sh
docker compose -f docker/compose.yml down -v      # -v also drops the peer-trust volumes
```
