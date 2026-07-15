"use client";

import { useRef, useState } from "react";
import { gettingStarted } from "@/content/getting-started";
import { site } from "@/content/site";
import { Section, SectionHeading, SectionKicker } from "@/components/ui/Section";
import { Reveal } from "@/components/motion/Reveal";
import { Button } from "@/components/ui/Button";
import { cn } from "@/lib/cn";

export function GettingStarted() {
  const tabs = gettingStarted.tabs;
  const [activeId, setActiveId] = useState<(typeof tabs)[number]["id"]>(tabs[0].id);
  const tabRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const activeTab = tabs.find((tab) => tab.id === activeId) ?? tabs[0];

  // Arrow-key roving between tabs, per the standard tablist pattern -- Tab/Enter alone already
  // reach and activate these (they're plain buttons), but a "tab" role implies this too.
  const handleKeyDown = (event: React.KeyboardEvent, index: number) => {
    if (event.key !== "ArrowRight" && event.key !== "ArrowLeft") return;
    event.preventDefault();
    const nextIndex =
      event.key === "ArrowRight" ? (index + 1) % tabs.length : (index - 1 + tabs.length) % tabs.length;
    setActiveId(tabs[nextIndex].id);
    tabRefs.current[nextIndex]?.focus();
  };

  return (
    <Section id="getting-started" aria-labelledby="getting-started-heading" muted>
      <Reveal>
        <SectionKicker>{gettingStarted.kicker}</SectionKicker>
        <SectionHeading id="getting-started-heading">{gettingStarted.heading}</SectionHeading>
        <p className="mt-6 text-lg text-fg-muted text-pretty">{gettingStarted.body}</p>
      </Reveal>

      <Reveal delay={0.08}>
        <div role="tablist" aria-label="Installation method" className="mt-10 flex gap-6 border-b border-border">
          {tabs.map((tab, index) => (
            <button
              key={tab.id}
              ref={(el) => {
                tabRefs.current[index] = el;
              }}
              role="tab"
              id={`getting-started-tab-${tab.id}`}
              aria-selected={tab.id === activeId}
              aria-controls={`getting-started-panel-${tab.id}`}
              tabIndex={tab.id === activeId ? 0 : -1}
              onClick={() => setActiveId(tab.id)}
              onKeyDown={(event) => handleKeyDown(event, index)}
              className={cn(
                "-mb-px border-b-2 pb-3 text-sm font-medium transition-colors",
                tab.id === activeId
                  ? "border-accent-strong text-fg"
                  : "border-transparent text-fg-muted hover:text-fg"
              )}
            >
              {tab.label}
            </button>
          ))}
        </div>
      </Reveal>

      <div role="tabpanel" id={`getting-started-panel-${activeTab.id}`} aria-labelledby={`getting-started-tab-${activeTab.id}`}>
        <ol className="mt-8 flex flex-col gap-8">
          {activeTab.steps.map((step, index) => (
            <Reveal key={step.title} delay={Math.min(index * 0.08, 0.24)}>
              <li className="flex gap-5">
                <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full border border-border font-mono text-xs text-accent-strong">
                  {index + 1}
                </span>
                <div className="min-w-0 flex-1">
                  <p className="font-medium">{step.title}</p>
                  <p className="mt-1 text-sm text-fg-muted">{step.detail}</p>
                  {step.code && (
                    <pre className="mt-3 overflow-x-auto rounded-lg bg-bg-elevated px-4 py-3 font-mono text-xs text-fg-muted">
                      <code>{step.code}</code>
                    </pre>
                  )}
                </div>
              </li>
            </Reveal>
          ))}
        </ol>
      </div>

      <Reveal delay={0.3}>
        <div className="mt-12 flex flex-col gap-3 sm:flex-row">
          <Button href={site.githubUrl} external>
            View on GitHub
          </Button>
          <Button href="#documentation" variant="secondary">
            Read the docs
          </Button>
        </div>
      </Reveal>
    </Section>
  );
}
