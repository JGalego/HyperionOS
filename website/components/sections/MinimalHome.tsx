"use client";

import { site } from "@/content/site";
import { minimalHome } from "@/content/minimal";
import { useMinimalMode } from "@/components/layout/MinimalModeProvider";
import { HyperionMark } from "@/components/ui/icons/HyperionMark";
import { ThemeToggle } from "@/components/ui/ThemeToggle";

// Deliberately nothing like the rest of the site: no starfield, no motion, no accent chrome --
// plain text and a short list of links, in the spirit of https://ggml.ai.
export function MinimalHome() {
  const { toggleMinimal } = useMinimalMode();

  return (
    <div className="minimal-view font-mono">
      <div className="mx-auto flex min-h-screen w-full max-w-md flex-col items-center justify-center gap-8 px-6 py-16 text-center">
        <HyperionMark className="h-8 w-auto text-fg" />

        <div>
          <p className="text-lg font-semibold">{site.name}</p>
          <p className="mt-1 text-sm text-fg-muted">{site.tagline}</p>
        </div>

        <p className="text-sm text-fg-muted">{minimalHome.body}</p>

        <ul className="flex flex-col gap-2 text-sm">
          {minimalHome.links.map((link) => (
            <li key={link.href}>
              <a
                href={link.href}
                target={link.href.startsWith("http") ? "_blank" : undefined}
                rel={link.href.startsWith("http") ? "noopener noreferrer" : undefined}
                className="underline decoration-border underline-offset-4 transition-colors hover:text-fg hover:decoration-fg"
              >
                {link.label}
              </a>
            </li>
          ))}
        </ul>

        <div className="flex items-center gap-4 text-xs text-fg-muted">
          <button
            type="button"
            onClick={toggleMinimal}
            className="underline decoration-border underline-offset-4 transition-colors hover:text-fg hover:decoration-fg"
          >
            {minimalHome.exitLabel}
          </button>
          <ThemeToggle />
        </div>
      </div>
    </div>
  );
}
