import { whyHyperion } from "@/content/why-hyperion";
import { Section, SectionHeading, SectionKicker } from "@/components/ui/Section";
import { Reveal } from "@/components/motion/Reveal";

export function WhyHyperion() {
  return (
    <Section id="why-hyperion" aria-labelledby="why-heading" muted>
      <Reveal>
        <SectionKicker>{whyHyperion.kicker}</SectionKicker>
        <SectionHeading id="why-heading">
          You shouldn&apos;t think like a{" "}
          <span className="text-accent-strong">computer</span>.
        </SectionHeading>
      </Reveal>

      <Reveal delay={0.1}>
        <ul className="mt-10 flex flex-wrap gap-3">
          {whyHyperion.frictions.map((friction) => (
            <li
              key={friction}
              className="rounded-full border border-border bg-bg-elevated px-4 py-2 text-sm text-fg-muted line-through decoration-fg-muted/50"
            >
              {friction}
            </li>
          ))}
        </ul>
      </Reveal>

      <Reveal delay={0.2}>
        <p className="mt-10 text-xl font-medium text-pretty">{whyHyperion.resolution}</p>
      </Reveal>
    </Section>
  );
}
