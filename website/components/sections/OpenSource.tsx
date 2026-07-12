import { openSource } from "@/content/open-source";
import { Section, SectionHeading, SectionKicker } from "@/components/ui/Section";
import { Reveal } from "@/components/motion/Reveal";
import { Button } from "@/components/ui/Button";

export function OpenSource() {
  return (
    <Section id="open-source" aria-labelledby="open-source-heading" muted>
      <Reveal>
        <SectionKicker>{openSource.kicker}</SectionKicker>
        <SectionHeading id="open-source-heading">{openSource.heading}</SectionHeading>

        <div className="mt-6 flex flex-col gap-4">
          {openSource.paragraphs.map((paragraph) => (
            <p key={paragraph} className="text-lg text-fg-muted text-pretty">
              {paragraph}
            </p>
          ))}
        </div>

        <div className="mt-8 flex flex-col gap-3 sm:flex-row">
          <Button href={openSource.primaryCta.href} external>
            {openSource.primaryCta.label}
          </Button>
          <Button href={openSource.secondaryCta.href} variant="secondary" external>
            {openSource.secondaryCta.label}
          </Button>
        </div>
      </Reveal>
    </Section>
  );
}
