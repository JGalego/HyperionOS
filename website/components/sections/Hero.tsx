import { hero } from "@/content/hero";
import { Button } from "@/components/ui/Button";
import { Container } from "@/components/ui/Container";
import { HeroMark } from "./HeroMark";

export function Hero() {
  return (
    <section
      id="top"
      aria-label="Introduction"
      className="flex min-h-[calc(100dvh-4rem)] items-center py-14"
    >
      <Container className="flex flex-col items-center text-center">
        <HeroMark />

        <h1 className="mt-8 text-3xl font-medium tracking-tight text-balance sm:text-4xl md:text-5xl">
          {hero.headline}
        </h1>

        <p className="mt-6 text-lg text-fg-muted text-pretty">{hero.subhead}</p>

        <div className="mt-8 flex flex-col gap-3 sm:flex-row">
          <Button href={hero.primaryCta.href}>{hero.primaryCta.label}</Button>
          <Button href={hero.secondaryCta.href} variant="secondary">
            {hero.secondaryCta.label}
          </Button>
        </div>
      </Container>
    </section>
  );
}
