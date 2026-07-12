import { features, featuresIntro } from "@/content/features";
import { Section, SectionHeading, SectionKicker } from "@/components/ui/Section";
import { Reveal } from "@/components/motion/Reveal";
import { cn } from "@/lib/cn";

export function Features() {
  return (
    <Section id="features" aria-labelledby="features-heading">
      <Reveal>
        <SectionKicker>{featuresIntro.kicker}</SectionKicker>
        <SectionHeading id="features-heading">{featuresIntro.heading}</SectionHeading>
      </Reveal>

      <div className="mt-12 grid gap-px overflow-hidden rounded-2xl border border-border bg-border sm:grid-cols-2">
        {features.map((feature, index) => {
          const isLastOdd = index === features.length - 1 && features.length % 2 === 1;
          return (
            <Reveal
              key={feature.title}
              delay={Math.min(index * 0.05, 0.2)}
              className={cn("h-full", isLastOdd && "sm:col-span-2")}
            >
              <div className="group h-full bg-bg-elevated p-8 transition-colors duration-300 hover:bg-bg-muted">
                <p className="text-lg font-medium text-pretty transition-colors group-hover:text-accent-strong">
                  {feature.title}
                </p>
                <p className="mt-3 text-sm text-fg-muted text-pretty">{feature.detail}</p>
              </div>
            </Reveal>
          );
        })}
      </div>
    </Section>
  );
}
