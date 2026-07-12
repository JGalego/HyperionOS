export const howItWorksIntro = {
  kicker: "How Hyperion works",
  heading: "You only need one sentence.",
  body: "You never pick an application. You say what you need, and Hyperion figures out the rest, visibly and explainably.",
};

export const traces = [
  {
    id: "interview",
    prompt: "Help me prepare for tomorrow's interview.",
    context: "Reads your calendar, your resume, and past notes. You don't repeat yourself.",
    assembled: ["Company research", "Practice questions", "Flashcards", "A timer", "The job posting"],
  },
  {
    id: "startup",
    prompt: "I need to launch my startup.",
    context: "Breaks one goal into a plan, and starts on what's ready first.",
    assembled: [
      "Market research",
      "Business plan",
      "Branding",
      "Entity formation",
      "A website",
      "Launch checklist",
    ],
  },
  {
    id: "graphic",
    prompt: "Create a social media graphic from these photos.",
    context: "Instead of: open Photoshop, open Finder, export PNG, save as.",
    assembled: ["Photos selected", "Layout composed", "Export ready"],
  },
] as const;
