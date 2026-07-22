"use client";

import { site } from "@/content/site";
import { philosophy } from "@/content/philosophy";
import { whyHyperion } from "@/content/why-hyperion";
import { architectureIntro, layers } from "@/content/architecture";
import { howItWorksIntro, traces } from "@/content/traces";
import { consoleDemosIntro, consoleDemos } from "@/content/console-demos";
import { featuresIntro, features } from "@/content/features";
import { gettingStarted } from "@/content/getting-started";
import { documentationIntro, documentationLinks } from "@/content/docs";
import { openSource } from "@/content/open-source";
import { minimalHome } from "@/content/minimal";
import { classifyConsoleLine, CONSOLE_STATUS_STYLE } from "@/lib/consoleStatus";
import { useMinimalMode } from "@/components/layout/MinimalModeProvider";
import { HyperionMark } from "@/components/ui/icons/HyperionMark";
import { ThemeToggle } from "@/components/ui/ThemeToggle";

function MinimalSection({
  id,
  kicker,
  heading,
  children,
}: {
  id: string;
  kicker: string;
  heading: string;
  children: React.ReactNode;
}) {
  return (
    <section id={id} className="border-t border-border py-10">
      <p className="text-xs uppercase tracking-widest text-fg-muted">{kicker}</p>
      <h2 className="mt-1 text-lg font-semibold text-fg">{heading}</h2>
      <div className="mt-4 flex flex-col gap-4 text-sm text-fg-muted">{children}</div>
    </section>
  );
}

function MinimalLink({ href, children }: { href: string; children: React.ReactNode }) {
  const external = href.startsWith("http");
  return (
    <a
      href={href}
      target={external ? "_blank" : undefined}
      rel={external ? "noopener noreferrer" : undefined}
      className="underline decoration-border underline-offset-4 transition-colors hover:text-fg hover:decoration-fg"
    >
      {children}
    </a>
  );
}

// Every full stop below renders the exact same text the animated site does -- pulled from the
// same content modules, in the same order the sections appear on the main page -- just as plain,
// left-aligned text instead of cards, tabs, and scroll-triggered reveals. "All text" is the whole
// point: nothing here is trimmed or summarized.
export function MinimalHome() {
  const { toggleMinimal } = useMinimalMode();

  return (
    <div className="minimal-view font-mono">
      <div className="sticky top-0 z-10 flex items-center justify-between border-b border-border bg-bg px-6 py-3 text-xs">
        <span className="font-semibold text-fg">{site.name}</span>
        <div className="flex items-center gap-4">
          <button
            type="button"
            onClick={toggleMinimal}
            className="text-fg-muted underline decoration-border underline-offset-4 transition-colors hover:text-fg hover:decoration-fg"
          >
            {minimalHome.exitLabel}
          </button>
          <ThemeToggle />
        </div>
      </div>

      <div className="mx-auto w-full max-w-2xl px-6 py-16">
        <div className="flex flex-col items-center gap-3 text-center">
          <HyperionMark className="h-8 w-auto text-fg" />
          <h1 className="text-lg font-semibold text-fg">{site.name}</h1>
          <p className="text-sm text-fg-muted">{site.tagline}</p>
          <p className="max-w-md text-sm text-fg-muted">{site.description}</p>
        </div>

        <MinimalSection id="philosophy" kicker={philosophy.kicker} heading={philosophy.heading}>
          <p>{philosophy.body}</p>
          <ul className="flex flex-col gap-1">
            {philosophy.comparison.map((row) => (
              <li key={row.before}>
                {row.before} → <span className="text-fg">{row.after}</span>
              </li>
            ))}
          </ul>
        </MinimalSection>

        <MinimalSection id="why-hyperion" kicker={whyHyperion.kicker} heading={whyHyperion.heading}>
          <ul className="flex flex-col gap-1">
            {whyHyperion.frictions.map((friction) => (
              <li key={friction} className="line-through decoration-fg-muted/50">
                {friction}
              </li>
            ))}
          </ul>
          <p className="text-fg">{whyHyperion.resolution}</p>
        </MinimalSection>

        <MinimalSection id="architecture" kicker={architectureIntro.kicker} heading={architectureIntro.heading}>
          <p>{architectureIntro.body}</p>
          <ul className="flex flex-col gap-4">
            {layers.map((layer) => (
              <li key={layer.id}>
                <p className="text-fg">
                  {layer.level} {layer.name} <span className="text-fg-muted">— {layer.plain}</span>
                </p>
                <p className="mt-1">{layer.description}</p>
                <p className="mt-1">{layer.detail}</p>
              </li>
            ))}
          </ul>
        </MinimalSection>

        <MinimalSection id="how-it-works" kicker={howItWorksIntro.kicker} heading={howItWorksIntro.heading}>
          <p>{howItWorksIntro.body}</p>
          <ul className="flex flex-col gap-4">
            {traces.map((trace) => (
              <li key={trace.id}>
                <p className="text-fg">&ldquo;{trace.prompt}&rdquo;</p>
                <p className="mt-1">{trace.context}</p>
                <p className="mt-1">Assembled: {trace.assembled.join(", ")}</p>
              </li>
            ))}
          </ul>
        </MinimalSection>

        <MinimalSection id="live-console" kicker={consoleDemosIntro.kicker} heading={consoleDemosIntro.heading}>
          <p>{consoleDemosIntro.body}</p>
          <ul className="flex flex-col gap-6">
            {consoleDemos.map((demo) => (
              <li key={demo.id}>
                <p className="text-fg">{demo.title}</p>
                <p className="mt-1">{demo.description}</p>
                <pre className="mt-3 overflow-x-auto whitespace-pre-wrap break-words rounded-lg bg-bg-muted p-4 text-[13px] leading-relaxed">
                  {demo.lines.map((line, index) => (
                    <span key={index} className="block">
                      {line.type === "input" ? (
                        <>
                          <span className="text-accent-strong">{"> "}</span>
                          <span className="text-fg">{line.text}</span>
                        </>
                      ) : (
                        (() => {
                          const status = classifyConsoleLine(line.text);
                          if (!status) return <span>{line.text}</span>;
                          const { className, glyph } = CONSOLE_STATUS_STYLE[status];
                          return (
                            <span className={className}>
                              {glyph} {line.text}
                            </span>
                          );
                        })()
                      )}
                    </span>
                  ))}
                </pre>
                <p className="mt-2">
                  <MinimalLink href={`${site.githubUrl}/blob/main/${demo.scenarioFile}`}>
                    {demo.scenarioFile}
                  </MinimalLink>
                </p>
              </li>
            ))}
          </ul>
        </MinimalSection>

        <MinimalSection id="features" kicker={featuresIntro.kicker} heading={featuresIntro.heading}>
          <ul className="flex flex-col gap-3">
            {features.map((feature) => (
              <li key={feature.title}>
                <p className="text-fg">{feature.title}</p>
                <p className="mt-1">{feature.detail}</p>
              </li>
            ))}
          </ul>
        </MinimalSection>

        <MinimalSection id="getting-started" kicker={gettingStarted.kicker} heading={gettingStarted.heading}>
          <p>{gettingStarted.body}</p>
          <div className="flex flex-col gap-6">
            {gettingStarted.tabs.map((tab) => (
              <div key={tab.id}>
                <p className="text-fg">{tab.label}</p>
                <ol className="mt-2 flex flex-col gap-3">
                  {tab.steps.map((step, index) => (
                    <li key={step.title}>
                      <p>
                        <span className="text-fg">
                          {index + 1}. {step.title}
                        </span>{" "}
                        — {step.detail}
                      </p>
                      {step.code && (
                        <pre className="mt-2 overflow-x-auto rounded-lg bg-bg-muted p-3 text-xs">
                          <code>{step.code}</code>
                        </pre>
                      )}
                    </li>
                  ))}
                </ol>
              </div>
            ))}
          </div>
          <p className="flex flex-wrap gap-x-4">
            <MinimalLink href={site.githubUrl}>View on GitHub</MinimalLink>
            <MinimalLink href="#documentation">Read the docs</MinimalLink>
          </p>
        </MinimalSection>

        <MinimalSection id="documentation" kicker={documentationIntro.kicker} heading={documentationIntro.heading}>
          <ul className="flex flex-col gap-3">
            {documentationLinks.map((link) => (
              <li key={link.href}>
                <p>
                  <MinimalLink href={link.href}>
                    <span className="text-fg">{link.title}</span>
                  </MinimalLink>{" "}
                  — {link.detail}
                </p>
              </li>
            ))}
          </ul>
        </MinimalSection>

        <MinimalSection id="open-source" kicker={openSource.kicker} heading={openSource.heading}>
          {openSource.paragraphs.map((paragraph) => (
            <p key={paragraph}>{paragraph}</p>
          ))}
          <p className="flex flex-wrap gap-x-4">
            <MinimalLink href={openSource.primaryCta.href}>{openSource.primaryCta.label}</MinimalLink>
            <MinimalLink href={openSource.secondaryCta.href}>{openSource.secondaryCta.label}</MinimalLink>
          </p>
        </MinimalSection>

        <div className="flex flex-col items-center gap-4 border-t border-border pt-10 text-center text-xs text-fg-muted">
          <ul className="flex flex-wrap justify-center gap-x-6 gap-y-2">
            {minimalHome.links.map((link) => (
              <li key={link.href}>
                <MinimalLink href={link.href}>{link.label}</MinimalLink>
              </li>
            ))}
          </ul>
          <p>
            © {new Date().getFullYear()} {site.name}. Open source under {site.license}.
          </p>
        </div>
      </div>
    </div>
  );
}
