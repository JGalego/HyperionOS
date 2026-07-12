import { stars as heroStars, twinkleStars, generateStars, generateShootingStars } from "@/content/starfield";

const shootingStars = generateShootingStars("hero-shooting");

// Dark-mode-only starfield. Plain positioned dots (not an SVG stretched to fill the box) so
// each star stays perfectly round no matter the container's aspect ratio. With no seed, this
// is the hero's own field -- the same one as the README banner. With a seed, it generates a
// distinct fixed field of `count` stars instead (e.g. the page-wide backdrop behind every card).
export function Starfield({ seed, count }: { seed?: string; count?: number } = {}) {
  const dots = seed ? generateStars(seed, count) : heroStars;

  return (
    <div className="pointer-events-none absolute inset-0 hidden overflow-hidden dark:block" aria-hidden="true">
      {dots.map((star, index) => (
        <span
          key={index}
          className="absolute rounded-full bg-fg"
          style={{
            left: `${star.x}%`,
            top: `${star.y}%`,
            width: `${star.r}px`,
            height: `${star.r}px`,
            opacity: star.o,
          }}
        />
      ))}
      {!seed &&
        twinkleStars.map((star, index) => (
          <span
            key={index}
            className="absolute rounded-full bg-fg motion-safe:animate-twinkle"
            style={{
              left: `${star.x}%`,
              top: `${star.y}%`,
              width: `${star.r}px`,
              height: `${star.r}px`,
              animationDuration: star.dur,
              animationDelay: star.delay,
            }}
          />
        ))}
      {!seed &&
        shootingStars.map((star, index) => (
          <span
            key={index}
            className="absolute h-[1.5px] w-[92px] rounded-full motion-safe:animate-shooting-star"
            style={{
              left: `${star.x}%`,
              top: `${star.y}%`,
              rotate: "19deg",
              opacity: 0,
              background: "linear-gradient(to right, transparent, var(--color-fg) 65%, var(--color-accent-strong) 100%)",
              animationDuration: star.duration,
              animationDelay: star.delay,
            }}
          />
        ))}
    </div>
  );
}
