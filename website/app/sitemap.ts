import type { MetadataRoute } from "next";
import { siteUrl } from "@/lib/seo";

// GitHub Pages only serves static files -- no server to render this on demand.
export const dynamic = "force-static";

export default function sitemap(): MetadataRoute.Sitemap {
  return [
    {
      url: siteUrl,
      lastModified: new Date(),
      changeFrequency: "weekly",
      priority: 1,
    },
  ];
}
