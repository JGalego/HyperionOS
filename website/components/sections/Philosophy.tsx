import { philosophy } from "@/content/philosophy";
import { Section, SectionHeading, SectionKicker } from "@/components/ui/Section";
import { Reveal } from "@/components/motion/Reveal";
import { ArrowRight } from "lucide-react";

export function Philosophy() {
  return (
    <Section id="philosophy" aria-labelledby="philosophy-heading">
      <Reveal>
        <SectionKicker>{philosophy.kicker}</SectionKicker>
        <SectionHeading id="philosophy-heading">
          Your <span className="text-accent-strong">goals</span> are the operating system.
        </SectionHeading>
        <p className="mt-6 text-lg text-fg-muted text-pretty">{philosophy.body}</p>
      </Reveal>

      <Reveal delay={0.1}>
        <dl className="mx-auto mt-12 max-w-2xl divide-y divide-border overflow-hidden rounded-2xl border border-border">
          {philosophy.comparison.map((row) => (
            <div
              key={row.before}
              className="grid grid-cols-[1fr_auto_1fr] items-center gap-4 bg-bg-elevated px-6 py-5"
            >
              <dt className="text-fg-muted">{row.before}</dt>
              <ArrowRight className="h-4 w-4 shrink-0 text-fg-muted" aria-hidden="true" />
              <dd className="text-right font-medium text-fg">{row.after}</dd>
            </div>
          ))}
        </dl>
      </Reveal>
    </Section>
  );
}
