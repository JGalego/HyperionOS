import { consoleDemosIntro, consoleDemos } from "@/content/console-demos";
import { Section, SectionHeading, SectionKicker } from "@/components/ui/Section";
import { Reveal } from "@/components/motion/Reveal";
import { cn } from "@/lib/cn";
import { ConsoleDemo } from "./ConsoleDemo";

export function LiveConsole() {
  return (
    <Section id="live-console" aria-labelledby="live-console-heading">
      <Reveal>
        <SectionKicker>{consoleDemosIntro.kicker}</SectionKicker>
        <SectionHeading id="live-console-heading">{consoleDemosIntro.heading}</SectionHeading>
        <p className="mt-6 text-lg text-fg-muted text-pretty">{consoleDemosIntro.body}</p>
      </Reveal>

      <div className="mt-12 flex flex-col gap-6">
        {consoleDemos.map((demo, index) => (
          <div key={demo.id} className={cn("md:w-[85%]", index % 2 === 1 && "md:ml-auto")}>
            <ConsoleDemo demo={demo} />
          </div>
        ))}
      </div>
    </Section>
  );
}
