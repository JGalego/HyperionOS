import Link from "next/link";
import { site, githubDoc } from "@/content/site";
import { navLinks } from "@/content/nav";
import { Container } from "@/components/ui/Container";
import { GithubMark } from "@/components/ui/icons/GithubMark";

const columns = [
  {
    heading: "Product",
    links: navLinks,
  },
  {
    heading: "Developers",
    links: [
      { label: "Getting started", href: "#getting-started" },
      { label: "SDK", href: githubDoc("25-sdk.md") },
      { label: "APIs", href: githubDoc("26-apis.md") },
      { label: "llms.txt", href: "/llms.txt" },
    ],
  },
  {
    heading: "Project",
    links: [
      { label: "Open source", href: "#open-source" },
      { label: "GitHub", href: site.githubUrl },
    ],
  },
];

export function Footer() {
  return (
    <footer className="relative z-10 border-t border-border">
      <Container className="grid gap-12 py-16 md:grid-cols-[1.2fr_repeat(3,1fr)]">
        <div>
          <p className="text-base font-semibold tracking-tight">{site.name}</p>
          <p className="mt-2 max-w-xs text-sm text-fg-muted">{site.tagline}</p>
          <a
            href={site.githubUrl}
            target="_blank"
            rel="noopener noreferrer"
            aria-label="Hyperion on GitHub"
            className="mt-4 inline-flex h-9 w-9 items-center justify-center rounded-full border border-border text-fg-muted transition-colors hover:text-fg"
          >
            <GithubMark className="h-4 w-4" aria-hidden="true" />
          </a>
        </div>

        {columns.map((column) => (
          <div key={column.heading}>
            <p className="font-mono text-xs uppercase tracking-widest text-fg-muted">
              {column.heading}
            </p>
            <ul className="mt-4 flex flex-col gap-3">
              {column.links.map((link) => (
                <li key={link.href}>
                  <Link
                    href={link.href}
                    className="text-sm text-fg-muted transition-colors hover:text-fg"
                  >
                    {link.label}
                  </Link>
                </li>
              ))}
            </ul>
          </div>
        ))}
      </Container>

      <Container className="flex flex-col gap-2 border-t border-border py-6 text-xs text-fg-muted sm:flex-row sm:items-center sm:justify-between">
        <p>
          © {new Date().getFullYear()} {site.name}. Open source under {site.license}.
        </p>
        <p>Humans express goals. Hyperion determines how those goals become reality.</p>
      </Container>
    </footer>
  );
}
