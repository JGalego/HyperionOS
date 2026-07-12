import { ChevronDown } from "lucide-react";
import type { layers } from "@/content/architecture";

type Layer = (typeof layers)[number];

export function ArchitectureLayer({ layer, isLast }: { layer: Layer; isLast: boolean }) {
  return (
    <div className="group relative pl-14">
      <span
        className="absolute left-0 top-1 flex h-9 w-9 items-center justify-center rounded-full border border-border bg-bg-elevated font-mono text-xs text-accent-strong transition-colors duration-200 group-hover:border-accent group-hover:bg-accent group-hover:text-accent-fg"
        aria-hidden="true"
      >
        {layer.level}
      </span>
      {!isLast && (
        <span
          className="absolute left-[17px] top-10 h-[calc(100%+1.5rem)] w-px bg-border"
          aria-hidden="true"
        />
      )}

      <details className="group/details pb-10">
        <summary className="flex cursor-pointer list-none items-start justify-between gap-4 [&::-webkit-details-marker]:hidden">
          <div>
            <p className="font-mono text-xs uppercase tracking-widest text-fg-muted transition-colors group-hover:text-accent-strong">
              {layer.name} · {layer.plain}
            </p>
            <p className="mt-2 text-lg text-fg">{layer.description}</p>
          </div>
          <ChevronDown
            className="mt-1 h-4 w-4 shrink-0 text-fg-muted transition-transform duration-200 group-open/details:rotate-180"
            aria-hidden="true"
          />
        </summary>
        <p className="mt-3 text-sm text-fg-muted text-pretty">{layer.detail}</p>
      </details>
    </div>
  );
}
