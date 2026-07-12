import path from "node:path";
import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  // An unrelated package-lock.json in the user's home directory otherwise makes Next.js guess
  // the wrong monorepo root; pin it to this project explicitly.
  turbopack: {
    root: path.join(__dirname),
  },
};

export default nextConfig;
