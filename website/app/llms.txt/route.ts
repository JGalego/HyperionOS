import { site } from "@/content/site";
import { philosophy } from "@/content/philosophy";
import { layers } from "@/content/architecture";
import { documentationLinks } from "@/content/docs";

// Machine-readable summary for LLMs per https://llmstxt.org, generated from the same content
// modules the page itself renders, so it can't silently drift from what's on the site.
function buildLlmsTxt(): string {
  const lines: string[] = [];

  lines.push(`# ${site.name}`, "", `> ${site.description}`, "");
  lines.push(
    philosophy.heading,
    "",
    "Hyperion is open source and still early: there is no installer, only a buildable, bootable reference implementation.",
    ""
  );

  lines.push("## Architecture");
  lines.push("");
  for (const layer of layers) {
    lines.push(`- ${layer.level} ${layer.name} (${layer.plain}): ${layer.description}`);
  }
  lines.push("");

  lines.push("## Docs");
  lines.push("");
  for (const link of documentationLinks) {
    lines.push(`- [${link.title}](${link.href}): ${link.detail}`);
  }
  lines.push("");

  lines.push("## Optional");
  lines.push("");
  lines.push(`- [Source code](${site.githubUrl}): the full monorepo, ${site.license} licensed.`);

  return lines.join("\n");
}

export const dynamic = "force-static";

export function GET() {
  return new Response(buildLlmsTxt(), {
    headers: {
      "Content-Type": "text/markdown; charset=utf-8",
    },
  });
}
