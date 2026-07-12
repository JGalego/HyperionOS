import { gettingStarted } from "@/content/getting-started";
import { site } from "@/content/site";
import { Section, SectionHeading, SectionKicker } from "@/components/ui/Section";
import { Reveal } from "@/components/motion/Reveal";
import { Button } from "@/components/ui/Button";

export function GettingStarted() {
  return (
    <Section id="getting-started" aria-labelledby="getting-started-heading" muted>
      <Reveal>
        <SectionKicker>{gettingStarted.kicker}</SectionKicker>
        <SectionHeading id="getting-started-heading">{gettingStarted.heading}</SectionHeading>
        <p className="mt-6 text-lg text-fg-muted text-pretty">{gettingStarted.body}</p>
      </Reveal>

      <ol className="mt-12 flex flex-col gap-8">
        {gettingStarted.steps.map((step, index) => (
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
