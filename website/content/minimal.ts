import { githubDoc, site } from "./site";

export const minimalHome = {
  body: "Every prior operating system asked you to manage proxies for what you wanted: a process, a file, a window, an application. Hyperion manages what you actually have.",
  exitLabel: "Full experience",
  links: [
    { label: "Documentation", href: githubDoc("00-index.md") },
    { label: "Architecture", href: githubDoc("02-core-architecture.md") },
    { label: "SDK", href: githubDoc("25-sdk.md") },
    { label: "GitHub", href: site.githubUrl },
    { label: "llms.txt", href: "/llms.txt" },
    { label: `License (${site.license})`, href: `${site.githubUrl}/blob/main/LICENSE` },
  ],
} as const;
