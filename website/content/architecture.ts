export const architectureIntro = {
  kicker: "Architecture",
  heading: "You speak. Everything else listens.",
  body: "Hyperion is built in layers, each one building on the one below it. Closer to the hardware, it's fast and safe. Closer to you, it understands what you mean.",
};

export const layers = [
  {
    id: "l0",
    level: "L0",
    name: "Kernel",
    plain: "Where it begins",
    description:
      "The hardware layer everything else stands on. It's open source, so its safety claims are checkable.",
    detail:
      "Capability-secured from the ground up: nothing crosses a trust boundary without an explicit, revocable grant.",
  },
  {
    id: "l1",
    level: "L1",
    name: "System runtime",
    plain: "How it stays fast and safe",
    description:
      "Schedules work across CPU, GPU, memory, and battery, keeping every process in its own boundary.",
    detail:
      "A unified scheduler balances compute, inference tokens, and context windows, the way earlier operating systems balanced CPU and RAM.",
  },
  {
    id: "l2",
    level: "L2",
    name: "Platform services",
    plain: "What it's built from",
    description: "Reusable capabilities, storage, updates, and networking.",
    detail:
      "Capabilities are Hyperion's replacement for the application: a declared contract, a trust level, and one or more interchangeable implementations.",
  },
  {
    id: "l3",
    level: "L3",
    name: "Knowledge",
    plain: "What it knows and remembers",
    description: "Your information, connected by meaning.",
    detail:
      "Every document, photo, message, or project is a Semantic Object with typed relationships to everything else: a knowledge graph standing in for folders and filenames.",
  },
  {
    id: "l4",
    level: "L4",
    name: "Cognition",
    plain: "Where it thinks",
    description:
      "Understands what you're asking for, recalls what's relevant, and picks the right model for the job.",
    detail:
      "The Intent Engine turns language into a graph of sub-goals. The Context Engine attaches what already exists (a calendar event, a past conversation). The Model Router picks local or cloud reasoning per task.",
  },
  {
    id: "l5",
    level: "L5",
    name: "Coordination",
    plain: "How the work gets organized",
    description:
      "When a goal needs more than one specialist, this layer decides who does what, and keeps a record of why.",
    detail:
      "Multi-agent orchestration assigns sub-goals to specialized agents and resolves conflicts between them. Every decision is logged for you to inspect.",
  },
  {
    id: "l6",
    level: "L6",
    name: "Experience",
    plain: "What you see and say",
    description:
      "Conversation, generated screens, and voice: the only parts of Hyperion you directly touch.",
    detail:
      "The Dynamic UI Runtime assembles a Workspace on demand for whatever you're doing, then tears it down when you're done. Accessibility is built into that runtime.",
  },
] as const;
