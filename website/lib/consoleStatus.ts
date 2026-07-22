/**
 * Mirrors crates/hyperion-console/src/color.rs's own classifier -- the same real, already-fixed
 * phrases hyperion-console itself recognizes at print time, ported here so this transcript reads
 * with the same green/amber/red a real terminal now shows. Never a bare substring match: anchored
 * on a line's own start or end, so a real model's own free-form prose is never misclassified (see
 * color.rs's own test for the exact case this guards against).
 */
export type ConsoleStatus = "success" | "warning" | "failure" | null;

function endsWithStatus(line: string, status: string): boolean {
  return line.startsWith("  ") && line.trimEnd().endsWith(`: ${status}`);
}

function isSuccess(line: string): boolean {
  return (
    line.startsWith("status: done --") ||
    line.includes(": Done --") ||
    line.startsWith("Switched to the ") ||
    line.startsWith("Connected (") ||
    endsWithStatus(line, "Done")
  );
}

function isFailure(line: string): boolean {
  return (
    line.startsWith("I couldn't") ||
    line.startsWith("I don't know") ||
    line.startsWith("I don't recognize") ||
    line.startsWith("I understood that, but couldn't") ||
    line.includes(": Failed --") ||
    endsWithStatus(line, "Failed")
  );
}

export function classifyConsoleLine(line: string): ConsoleStatus {
  if (isSuccess(line)) return "success";
  if (line.startsWith("warning:") || endsWithStatus(line, "Blocked")) return "warning";
  if (isFailure(line)) return "failure";
  return null;
}

/** Same green/amber/red-and-glyph treatment `crates/hyperion-console/src/color.rs` gives a real
 * terminal -- shared by every UI that renders a classified console line (the animated
 * ConsoleDemo and the plain-text MinimalHome alike), so the two can't quietly drift apart. */
export const CONSOLE_STATUS_STYLE: Record<Exclude<ConsoleStatus, null>, { className: string; glyph: string }> = {
  success: { className: "text-console-success", glyph: "✓" },
  warning: { className: "text-console-warning", glyph: "⚠" },
  failure: { className: "text-console-failure", glyph: "✗" },
};
