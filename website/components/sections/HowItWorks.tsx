import { howItWorksIntro, traces } from "@/content/traces";
import { Section, SectionHeading, SectionKicker } from "@/components/ui/Section";
import { Reveal } from "@/components/motion/Reveal";
import { cn } from "@/lib/cn";
import { IntentTrace } from "./IntentTrace";

export function HowItWorks() {
  return (
    <Section id="how-it-works" aria-labelledby="how-it-works-heading" muted>
      <Reveal>
        <SectionKicker>{howItWorksIntro.kicker}</SectionKicker>
        <SectionHeading id="how-it-works-heading">{howItWorksIntro.heading}</SectionHeading>
        <p className="mt-6 text-lg text-fg-muted text-pretty">{howItWorksIntro.body}</p>
      </Reveal>

      <div className="mt-12 flex flex-col gap-6">
        {traces.map((trace, index) => (
          <div key={trace.id} className={cn("md:w-[85%]", index % 2 === 1 && "md:ml-auto")}>
            <IntentTrace trace={trace} />
          </div>
        ))}
      </div>
    </Section>
  );
}
