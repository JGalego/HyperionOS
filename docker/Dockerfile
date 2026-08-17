# A Hyperion node, as small as a container can be.
#
# `hyperion-console` cross-compiles to a fully static-pie musl binary, so the runtime image needs
# no distro at all -- no libc, no shell, no package manager, and so no distro CVE surface to patch.
# Two files: the binary, and the CA bundle its TLS stack needs to reach a cloud model provider.
#
# Build the binary first (see docker/build.sh, which is what compose calls):
#   CC_x86_64_unknown_linux_musl=musl-gcc cargo build --release \
#     --target x86_64-unknown-linux-musl -p hyperion-console --bin hyperion-console \
#     --features real-http,openai-compat,anthropic,gemini,mdns
FROM scratch

# Only needed to reach a cloud provider over TLS; the mesh itself is plain HTTP between peers,
# and the default mock backend needs no network at all.
COPY ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY hyperion-console /hyperion-console

# Each node keeps its own knowledge graph, peer-trust file and WAL here. Compose gives every node
# its own volume so restarting one doesn't hand it another node's memory.
ENV HYPERION_CONSOLE_DATA_DIR=/var/lib/hyperion

# A2A (peer-to-peer delegation), MCP (tool access), and the live mesh dashboard.
EXPOSE 8600 8700 8800

ENTRYPOINT ["/hyperion-console"]
