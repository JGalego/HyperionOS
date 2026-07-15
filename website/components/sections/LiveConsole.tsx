import { consoleDemosIntro, consoleDemos } from "@/content/console-demos";
import { Section, SectionHeading, SectionKicker } from "@/components/ui/Section";
import { Reveal } from "@/components/motion/Reveal";
import { ConsoleDemo } from "./ConsoleDemo";

export function LiveConsole() {
  return (
    <Section id="live-console" aria-labelledby="live-console-heading">
      <Reveal>
        <SectionKicker>{consoleDemosIntro.kicker}</SectionKicker>
        <SectionHeading id="live-console-heading">{consoleDemosIntro.heading}</SectionHeading>
        <p className="mt-6 text-lg text-fg-muted text-pretty">{consoleDemosIntro.body}</p>
      </Reveal>

      <div className="mt-12 grid gap-6 lg:grid-cols-3">
        {consoleDemos.map((demo) => (
          <ConsoleDemo key={demo.id} demo={demo} />
        ))}
      </div>
    </Section>
  );
}
