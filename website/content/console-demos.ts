// Real hyperion-console transcript excerpts -- not illustrative copy like content/traces.ts above.
// Every line here is verbatim output from an actual run of the named scenario file (see
// scenarios/ and docs/999-usage-scenarios.md), trimmed to a representative excerpt the same way
// hyperion-console's own graph_explorer.rs truncates a long result with "..." rather than
// inventing a shorter fake one. Regenerate by re-running the scenario file and re-pasting its
// real output, not by hand-editing these lines.

export const consoleDemosIntro = {
  kicker: "Real sessions, not mockups",
  heading: "This actually ran.",
  body: "Every transcript below is real hyperion-console output from a real, checked-in scenario file -- the same ones this repo's own CI and docs/999-usage-scenarios.md verify against. Nothing here is staged copy.",
};

export type ConsoleLine = { type: "input" | "output"; text: string };

export const consoleDemos = [
  {
    id: "onboarding",
    title: "A new user's first session",
    description: 'Conversational continuity, the decomposed "launch my startup" plan, then a named result lookup.',
    scenarioFile: "scenarios/onboarding-new-user-launches-startup.txt",
    lines: [
      { type: "input", text: "my name is Alex" },
      { type: "output", text: "status: done -- [mock model 1] echo: my name is Alex" },
      { type: "input", text: "what is my name" },
      {
        type: "output",
        text: 'status: done -- [mock model 1] echo: Recent conversation, most recent last:\nmy name is Alex\n\nNow respond to: what is my name',
      },
      { type: "input", text: "launch my startup" },
      {
        type: "output",
        text: "  market_research: Done\n  business_model: Done\n  branding: Done\n  legal_formation: Done",
      },
      { type: "input", text: "/result market_research" },
      {
        type: "output",
        text: '"market_research"\'s real result, recorded just now:\n\n[mock model 1] echo: Provide a concise research summary about market_research -- launch my startup. ...',
      },
    ] satisfies ConsoleLine[],
  },
  {
    id: "knowledge-graph",
    title: "Watching the knowledge graph grow",
    description: "An empty graph, a real action that grows it, then the real nodes and edges it recorded.",
    scenarioFile: "scenarios/knowledge-graph-growth-audit.txt",
    lines: [
      { type: "input", text: "/graph" },
      { type: "output", text: "The knowledge graph is empty -- nothing recorded yet." },
      { type: "input", text: "launch my startup" },
      {
        type: "output",
        text: "  market_research: Done\n  business_model: Done\n  branding: Done\n  legal_formation: Done",
      },
      { type: "input", text: "/graph" },
      {
        type: "output",
        text: '9 nodes:\n  [0] intent -- you asked: "launch my startup" (90% confident)\n  [1] intent -- a planned task: market_research\n  [2] intent -- a planned task: business_model\n  [3] intent -- a planned task: branding\n  [4] intent -- a planned task: legal_formation\n  [8] task_result -- a result: [mock model 1] echo: Provide a concise research summary...\n\n7 edges:\n  [5] 2 --depends_on--> 1 (weight 1.00)\n  [9] 1 --produced--> 8 (weight 1.00)',
      },
    ] satisfies ConsoleLine[],
  },
  {
    id: "cloud-providers",
    title: "Three real cloud providers, one question",
    description: "OpenAI, Anthropic, and Groq, connected and switched live in the same session, each asked the same real question.",
    scenarioFile: "scenarios/cloud-provider-comparison.txt",
    lines: [
      { type: "input", text: "/backend openai gpt-4o-mini" },
      { type: "output", text: 'Switched to the openai (model "gpt-4o-mini") backend.' },
      { type: "input", text: "what is the smallest country in the world by land area" },
      {
        type: "output",
        text: "status: done -- The smallest country in the world by land area is Vatican City. It has an area of approximately 44 hectares (110 acres) or about 0.17 square miles...",
      },
      { type: "input", text: "/backend anthropic claude-haiku-4-5-20251001" },
      { type: "output", text: 'Switched to the anthropic (model "claude-haiku-4-5-20251001") backend.' },
      { type: "input", text: "what is the smallest country in the world by land area" },
      {
        type: "output",
        text: "status: done -- # The Smallest Country in the World by Land Area\n\n**Vatican City** is the smallest country in the world by land area, with only about **0.44 square kilometers**...",
      },
      { type: "input", text: "/backend groq llama-3.3-70b-versatile" },
      { type: "output", text: 'Switched to the groq (model "llama-3.3-70b-versatile") backend.' },
      { type: "input", text: "what is the smallest country in the world by land area" },
      {
        type: "output",
        text: "status: done -- The smallest country in the world by land area is the Vatican City, with an area of approximately 0.44 km² (0.17 sq mi)...",
      },
    ] satisfies ConsoleLine[],
  },
] as const;
