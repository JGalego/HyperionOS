export const hero = {
  headline: "You ask. It understands.",
  subhead: "Hyperion is the first intent-native operating system.",
  primaryCta: { label: "Get started", href: "#getting-started" },
  secondaryCta: { label: "See how it works", href: "#how-it-works" },
  // A fair complaint: this ring times a fixed animation, it doesn't track real work. Answered
  // in the same five questions CLAUDE.md demands of every autonomous action (Why/How/Evidence/
  // Confidence/Undo), because the honest answer is funnier than pretending otherwise.
  progressEasterEgg:
    "Why: feels more responsive than a blank screen. How: a fixed 5-second ease-out timer, not real work. Evidence: LoaderProvider.tsx. Confidence: 100% (it always finishes). Undo: refresh.",
} as const;
