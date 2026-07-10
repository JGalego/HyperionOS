# Hyperion Engineering Specification — Index

**Hyperion** is an intent-native operating system: humans express goals, and the system determines
how those goals become reality. This index is the entry point to the full specification —
42 documents covering vision, architecture, every subsystem, and the plan to build it.

If you are new to this specification, read [01](01-vision-and-philosophy.md) and
[02](02-core-architecture.md) first — every other document assumes their vocabulary (Intent,
Capability, Semantic Object, Context Bundle, Workspace, Agent, Trust Boundary) and their 7-layer
architecture (L0 Kernel → L6 Experience). After that, read top-down (start from what the user
experiences, in §3 below) or bottom-up (start from the kernel, in §2), or jump directly to a
subsystem via the tables below.

## 1. Foundations

| Doc | Title | What it covers |
|---|---|---|
| [01](01-vision-and-philosophy.md) | Vision & Philosophy | The Golden Rule, universal usability, adaptive complexity, human-language-first design, human control, and success criteria |
| [02](02-core-architecture.md) | Core Architecture | The 7-layer system stack, shared vocabulary, a worked request trace, design invariants, and capability security as the unifying model |

## 2. System Runtime & Kernel (Layer L0-L1)

| Doc | Title | What it covers |
|---|---|---|
| [03](03-kernel-architecture.md) | Kernel Architecture | Hybrid microkernel design, HAL, driver model, capability monitor, sandboxing/virtualization/container spectrum |
| [04](04-scheduler.md) | Scheduler | Unified scheduling across CPU/GPU/RAM/VRAM/storage/battery/network/inference-tokens/context-windows |
| [30](30-ipc-framework.md) | IPC Framework | Capability-scoped message passing, wire protocol, zero-copy, transparent local/remote routing |
| [31](31-event-system.md) | Event System | System-wide pub/sub backbone for object-changed, progress, and device events |

## 3. Knowledge & Data (Layer L2-L3)

| Doc | Title | What it covers |
|---|---|---|
| [28](28-storage-engine.md) | Storage Engine | WAL-backed blob + graph + vector + metadata store, replication, encryption at rest |
| [29](29-database-schema.md) | Database Schema | Concrete object/edge/version schema, sharding, worked examples |
| [09](09-knowledge-graph.md) | Knowledge Graph | Semantic Object graph model, embeddings, reasoning-as-search |
| [10](10-semantic-filesystem.md) | Semantic Filesystem | Query-as-navigation view layer, POSIX compatibility shim |

## 4. Cognition (Layer L4)

| Doc | Title | What it covers |
|---|---|---|
| [05](05-intent-engine.md) | Intent Engine | Natural language → Intent Graph, decomposition, reconciliation |
| [06](06-context-engine.md) | Context Engine | Context Bundle assembly, relevance ranking, Adaptive Complexity signal |
| [07](07-context-propagation.md) | Context Propagation | Wire format, cross-device/cross-boundary propagation, staleness |
| [08](08-memory-engine.md) | Memory Engine | Working/episodic/semantic/procedural/long-term memory tiers, decay, transparency |
| [22](22-local-ai-runtime.md) | Local AI Runtime | On-device model execution, hardware-adaptive quantization, resident model management |
| [23](23-multi-model-orchestration.md) | Multi-Model Orchestration | The Model Router — routing algorithm, ensemble verification, staged rollout |

## 5. Agents & Coordination (Layer L4-L5)

| Doc | Title | What it covers |
|---|---|---|
| [11](11-agent-runtime.md) | Agent Runtime | Sandboxed Agent process model, lifecycle, built-in specializations |
| [12](12-multi-agent-coordination.md) | Multi-Agent Coordination | Task allocation, shared-plan blackboard, conflict resolution, failure containment |

## 6. Experience (Layer L6)

| Doc | Title | What it covers |
|---|---|---|
| [13](13-dynamic-ui-runtime.md) | Dynamic UI Runtime | Compiler-pipeline Workspace generation from Intent + Capabilities |
| [14](14-accessibility.md) | Accessibility | Accessibility tree as a first-class compiler output, not a bolt-on |

## 7. Trust: Security, Privacy, Explainability

| Doc | Title | What it covers |
|---|---|---|
| [15](15-security-architecture.md) | Security Architecture | Cross-layer capability tokens, sandboxing enforcement, intent-aware risk assessment |
| [16](16-privacy-architecture.md) | Privacy Architecture | Privacy tiers, encryption, end-to-end sync, inspect/edit/export/erase |
| [17](17-threat-model.md) | Threat Model | Attacker-goal/mitigation tables for the novel attack surfaces of an intent-native OS |
| [18](18-explainability-and-trust.md) | Explainability & Trust | Explanation Records, why/evidence/confidence/alternatives/undo, interruption control |

## 8. Networking, Devices & Distribution

| Doc | Title | What it covers |
|---|---|---|
| [19](19-networking-stack.md) | Networking Stack | Semantic web layer — entities instead of URLs |
| [20](20-device-framework.md) | Device Framework | Device Objects, capability-secured pairing, cross-device Workspaces |
| [21](21-distributed-execution.md) | Distributed Execution | Device federation, work placement, state migration, offline consistency |

## 9. Platform & Developer Surface

| Doc | Title | What it covers |
|---|---|---|
| [24](24-plugin-framework.md) | Plugin Framework | Capability manifest format, sandboxed installation, registry |
| [25](25-sdk.md) | SDK | Capability development tooling, local emulator, testing harness, publishing |
| [26](26-apis.md) | APIs | Intent/Context/Memory/Knowledge-Graph/Capability-Invocation API contracts |
| [27](27-compatibility-layer.md) | Compatibility Layer | Windows/Linux/Android/Web/CLI support via VM/container Trust Boundaries |

## 10. Lifecycle & Operations

| Doc | Title | What it covers |
|---|---|---|
| [32](32-update-system.md) | Update System | Atomic OS/Capability/model updates, staged rollout, canary gating |
| [33](33-rollback-recovery.md) | Rollback & Recovery | Recovery points, undo at action/session/Agent/goal scope, crash recovery |
| [34](34-observability-telemetry.md) | Observability & Telemetry | Metrics/logs/traces, tamper-evident audit ledger, privacy-respecting telemetry |
| [35](35-testing-strategy.md) | Testing Strategy | Layered testing from deterministic kernel tests to adversarial multi-agent chaos testing |

## 11. Performance & Scale

| Doc | Title | What it covers |
|---|---|---|
| [36](36-performance-benchmarks.md) | Performance Benchmarks | Cold boot, wake, workspace-generation, and inference targets with measurement methodology |
| [37](37-scalability-roadmap.md) | Scalability Roadmap | Raspberry-Pi-class to enterprise-cluster scaling, degradation strategy |

## 12. Business & Governance

| Doc | Title | What it covers |
|---|---|---|
| [38](38-five-year-evolution.md) | Five-Year Evolution Plan | Year-by-year roadmap grouping the ten implementation phases |
| [39](39-commercial-strategy.md) | Commercial Strategy | Revenue model consistent with local-first, privacy-first principles |
| [40](40-open-source-governance.md) | Open-Source Governance | Core-vs-ecosystem boundary, RFC process, certification program |

## 13. Implementation Plan

| Doc | Title | What it covers |
|---|---|---|
| [41](41-implementation-phases.md) | Implementation Phases | The ten-phase build sequence, entry/exit criteria, and risks for every phase |

---
*Start with [01 — Vision & Philosophy](01-vision-and-philosophy.md).*
