// Transcript lines below are copied verbatim from an actual run of the named scenario file (see
// scenarios/), not written for the page. Regenerate by re-running that file and pasting its
// output back in, rather than editing the lines by hand.

export const consoleDemosIntro = {
  kicker: "Live console",
  heading: "You can watch it run.",
  body: "Every session below is copied from an actual hyperion-console run. Nothing here was written for the page.",
};

export type ConsoleLine = { type: "input" | "output"; text: string };

export const consoleDemos = [
  {
    id: "onboarding",
    title: "A new user's first session",
    description:
      "Hyperion remembers what you told it a moment ago, breaks one goal into several tasks, and can hand back any single result by name.",
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
    description:
      "One command shows what Hyperion has learned, before and after it acts, as a graph you can inspect yourself.",
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
    title: "One question, three models",
    description:
      "OpenAI, Anthropic, and Groq each answer the same question in the same session, switched live with no restart.",
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
  {
    id: "social-mesh",
    title: "A node that can't translate, asks one that can",
    description:
      "Six real, separately-launched hyperion-console processes discover each other over real mDNS, each backed by a different real cloud model (Anthropic, OpenAI, Groq) -- no human ever names a host, port, or peer.",
    scenarioFile: "scripts/run-mesh-demo.sh",
    lines: [
      { type: "input", text: "/backend groq llama-3.1-8b-instant" },
      { type: "output", text: 'Switched to the groq (model "llama-3.1-8b-instant") backend.' },
      { type: "input", text: "/mesh-request 9106 translate-ja say hello in Japanese" },
      {
        type: "output",
        text: 'I don\'t have "translate-ja" myself, so I asked kenji._hyperion-a2a._tcp.local. (127.0.0.1:9101), which does. It replied: status: done -- こんにちは (Konnichiwa)\n\nThis is a common way to say "hello" in Japanese, used during daytime hours. Other greetings include:\n- おはよう (Ohayou) - good morning\n- こんばんは (Konbanwa) - good evening\n\n(Trusting 127.0.0.1:9101\'s identity for the first time: 46c559994406105160c06ed3e3182cbe229d2dd27dee297afc2fbf70745dc0d1.)',
      },
    ] satisfies ConsoleLine[],
  },
] as const;
