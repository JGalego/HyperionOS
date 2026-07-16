import path from "node:path";
import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  // An unrelated package-lock.json in the user's home directory otherwise makes Next.js guess
  // the wrong monorepo root; pin it to this project explicitly.
  turbopack: {
    root: path.join(__dirname),
  },
  // GitHub Pages only serves static files -- no Node server for SSR/ISR or next/image's own
  // optimizer. `output: "export"` builds this whole site to plain HTML/CSS/JS in `out/`, which
  // `.github/workflows/deploy-pages.yml` then publishes as-is. Served from a real custom domain
  // (try-hyperion.org, not a github.io/<repo> subpath), so no basePath is needed.
  output: "export",
};

export default nextConfig;
