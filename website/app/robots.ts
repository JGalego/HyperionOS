import type { MetadataRoute } from "next";
import { siteUrl } from "@/lib/seo";

// GitHub Pages only serves static files -- no server to render this on demand.
export const dynamic = "force-static";

export default function robots(): MetadataRoute.Robots {
  return {
    rules: {
      userAgent: "*",
      allow: "/",
    },
    sitemap: `${siteUrl}/sitemap.xml`,
  };
}
