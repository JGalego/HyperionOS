import { site } from "./site";

// Every section's actual text lives in its own content module (philosophy.ts, traces.ts, and so
// on) -- MinimalHome renders those directly so the plain-text page can't drift from the real one.
// This file holds only what's specific to the minimal page itself: the exit control's label and
// the site-level (not doc-specific) links its footer lists.
export const minimalHome = {
  exitLabel: "Full experience",
  links: [
    { label: "GitHub", href: site.githubUrl },
    { label: "llms.txt", href: "/llms.txt" },
    { label: `License (${site.license})`, href: `${site.githubUrl}/blob/main/LICENSE` },
  ],
} as const;
