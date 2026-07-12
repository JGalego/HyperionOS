import { githubDoc, site } from "./site";

export const openSource = {
  kicker: "Open source",
  heading: "You can verify every claim we make",
  paragraphs: [
    "Trusting an operating system with your goals, your memory, and the authority to act on your behalf can't rest on a vendor's word alone: it has to be checkable. The kernel, the scheduler, the IPC layer, and the platform services underneath everything are open source from day one, under the MIT license.",
  ],
  primaryCta: { label: "Read the governance model", href: githubDoc("40-open-source-governance.md") },
  secondaryCta: { label: "Contribute on GitHub", href: site.githubUrl },
} as const;
