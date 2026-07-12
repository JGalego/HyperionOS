import { architectureIntro, layers } from "@/content/architecture";
import { Section, SectionHeading, SectionKicker } from "@/components/ui/Section";
import { Reveal } from "@/components/motion/Reveal";
import { ArchitectureLayer } from "./ArchitectureLayer";

export function Architecture() {
  return (
    <Section id="architecture" aria-labelledby="architecture-heading">
      <Reveal>
        <SectionKicker>{architectureIntro.kicker}</SectionKicker>
        <SectionHeading id="architecture-heading">{architectureIntro.heading}</SectionHeading>
        <p className="mt-6 text-lg text-fg-muted text-pretty">{architectureIntro.body}</p>
      </Reveal>

      <div className="mt-14">
        {layers.map((layer, index) => (
          <Reveal key={layer.id} delay={Math.min(index * 0.05, 0.3)}>
            <ArchitectureLayer layer={layer} isLast={index === layers.length - 1} />
          </Reveal>
        ))}
      </div>
    </Section>
  );
}
