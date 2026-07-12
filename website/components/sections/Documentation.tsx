import { ArrowUpRight } from "lucide-react";
import { documentationIntro, documentationLinks } from "@/content/docs";
import { Section, SectionHeading, SectionKicker } from "@/components/ui/Section";
import { Reveal } from "@/components/motion/Reveal";

export function Documentation() {
  return (
    <Section id="documentation" aria-labelledby="documentation-heading">
      <Reveal>
        <SectionKicker>{documentationIntro.kicker}</SectionKicker>
        <SectionHeading id="documentation-heading">{documentationIntro.heading}</SectionHeading>
      </Reveal>

      <div className="mt-12 grid gap-4 sm:grid-cols-2">
        {documentationLinks.map((link, index) => (
          <Reveal key={link.title} delay={Math.min(index * 0.05, 0.2)}>
            <a
              href={link.href}
              target="_blank"
              rel="noopener noreferrer"
              className="group flex h-full flex-col justify-between rounded-2xl border border-border p-6 transition-all duration-200 hover:-translate-y-0.5 hover:border-accent-strong hover:bg-bg-muted"
            >
              <div>
                <div className="flex items-center justify-between gap-4">
                  <p className="font-medium">{link.title}</p>
                  <ArrowUpRight
                    className="h-4 w-4 shrink-0 text-fg-muted transition-transform group-hover:translate-x-0.5 group-hover:-translate-y-0.5"
                    aria-hidden="true"
                  />
                </div>
                <p className="mt-2 text-sm text-fg-muted">{link.detail}</p>
              </div>
            </a>
          </Reveal>
        ))}
      </div>
    </Section>
  );
}
