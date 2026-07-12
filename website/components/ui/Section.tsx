import { cn } from "@/lib/cn";
import { Reveal } from "@/components/motion/Reveal";
import { Container } from "./Container";

export function Section({
  id,
  className,
  containerClassName,
  cardClassName,
  muted = false,
  children,
  "aria-labelledby": ariaLabelledBy,
}: {
  id?: string;
  className?: string;
  containerClassName?: string;
  cardClassName?: string;
  muted?: boolean;
  children: React.ReactNode;
  "aria-labelledby"?: string;
}) {
  return (
    <section id={id} aria-labelledby={ariaLabelledBy} className={cn("px-4 py-3 md:px-6", className)}>
      <Container className={containerClassName}>
        <Reveal>
          <div
            className={cn(
              "rounded-3xl border border-border px-6 py-12 sm:px-10 sm:py-16 md:px-12 md:py-20 lg:px-16 lg:py-24",
              muted ? "bg-bg-muted" : "bg-bg-elevated",
              cardClassName
            )}
          >
            {children}
          </div>
        </Reveal>
      </Container>
    </section>
  );
}

export function SectionKicker({ children }: { children: React.ReactNode }) {
  return (
    <p className="font-mono text-sm uppercase tracking-widest text-accent-strong">{children}</p>
  );
}

export function SectionHeading({
  id,
  className,
  children,
}: {
  id?: string;
  className?: string;
  children: React.ReactNode;
}) {
  return (
    <h2
      id={id}
      className={cn(
        "mt-3 text-3xl font-medium tracking-tight text-balance sm:text-4xl md:text-5xl",
        className
      )}
    >
      {children}
    </h2>
  );
}
