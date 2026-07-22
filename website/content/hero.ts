export const hero = {
  headline: "You ask. It understands.",
  subhead: "Hyperion is the first intent-native operating system.",
  primaryCta: { label: "Get started", href: "#getting-started" },
  secondaryCta: { label: "See how it works", href: "#how-it-works" },
  // A fair complaint: this ring times a fixed animation, it doesn't track real work. Escalates
  // the more you click it -- past the last line, Hero.tsx switches to a running click count,
  // which is the one number in this whole component that's actually real.
  progressEasterEgg: [
    "You found it. It's not tracking anything real. Happy now?",
    "You clicked it again. It's still fake. It was fake the first time too.",
    "Third click. You've now spent more effort on this than the animation did.",
    "There's nothing else here. This is the whole joke.",
  ],
} as const;
