import { githubDoc } from "./site";

export const documentationIntro = {
  kicker: "Documentation",
  heading: "You can check our work.",
};

export const documentationLinks = [
  {
    title: "Architecture",
    detail: "The full system, from kernel to conversation.",
    href: githubDoc("02-core-architecture.md"),
  },
  {
    title: "Developer SDK",
    detail: "Build a Capability: local emulator, testing harness, publishing.",
    href: githubDoc("25-sdk.md"),
  },
  {
    title: "APIs",
    detail: "Intent, Context, Memory, Knowledge Graph, and Capability invocation contracts.",
    href: githubDoc("26-apis.md"),
  },
  {
    title: "Community",
    detail: "Issues, discussion, and the state of every subsystem, in one repository.",
    href: "https://github.com/JGalego/HyperionOS",
  },
] as const;
