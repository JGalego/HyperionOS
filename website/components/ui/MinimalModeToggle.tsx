"use client";

import { AlignLeft, Sparkles } from "lucide-react";
import { useMinimalMode } from "@/components/layout/MinimalModeProvider";

export function MinimalModeToggle() {
  const { minimal, toggleMinimal } = useMinimalMode();

  return (
    <button
      type="button"
      aria-label="Toggle minimal mode"
      aria-pressed={minimal}
      title="Minimal mode"
      className="flex h-9 w-9 items-center justify-center rounded-full text-fg-muted transition-colors hover:bg-bg-muted hover:text-fg"
      onClick={toggleMinimal}
    >
      {minimal ? (
        <Sparkles className="h-4 w-4" aria-hidden="true" />
      ) : (
        <AlignLeft className="h-4 w-4" aria-hidden="true" />
      )}
    </button>
  );
}
