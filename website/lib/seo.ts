import { site } from "@/content/site";

// No production domain is deployed yet; NEXT_PUBLIC_SITE_URL lets a real deployment set the
// canonical origin without this code guessing at a domain nobody has registered.
export const siteUrl = process.env.NEXT_PUBLIC_SITE_URL ?? "http://localhost:3000";

export function jsonLd() {
  return {
    "@context": "https://schema.org",
    "@type": "SoftwareApplication",
    name: site.name,
    description: site.description,
    applicationCategory: "OperatingSystem",
    operatingSystem: "Hyperion",
    url: site.githubUrl,
    codeRepository: site.githubUrl,
    license: "https://opensource.org/license/mit/",
    isAccessibleForFree: true,
  };
}
