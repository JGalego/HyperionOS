import type { Metadata } from "next";
import { Geist, Geist_Mono } from "next/font/google";
import { site } from "@/content/site";
import { siteUrl, jsonLd } from "@/lib/seo";
import { ThemeProvider } from "@/components/layout/ThemeProvider";
import { NavBar } from "@/components/layout/NavBar";
import { Footer } from "@/components/layout/Footer";
import { Starfield } from "@/components/ui/Starfield";
import "./globals.css";

const geistSans = Geist({
  variable: "--font-geist-sans",
  subsets: ["latin"],
});

const geistMono = Geist_Mono({
  variable: "--font-geist-mono",
  subsets: ["latin"],
});

export const metadata: Metadata = {
  metadataBase: new URL(siteUrl),
  title: {
    default: `${site.name}: ${site.tagline}`,
    template: `%s: ${site.name}`,
  },
  description: site.description,
  keywords: ["Hyperion", "intent-native operating system", "AI operating system", "open source OS"],
  authors: [{ name: site.name }],
  openGraph: {
    type: "website",
    title: `${site.name}: ${site.tagline}`,
    description: site.description,
    siteName: site.name,
  },
  twitter: {
    card: "summary_large_image",
    title: `${site.name}: ${site.tagline}`,
    description: site.description,
  },
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html
      lang="en"
      suppressHydrationWarning
      className={`${geistSans.variable} ${geistMono.variable} antialiased`}
    >
      <body className="relative flex min-h-screen flex-col">
        <script
          type="application/ld+json"
          dangerouslySetInnerHTML={{ __html: JSON.stringify(jsonLd()) }}
        />

        {/* The page's own starfield, behind every card -- visible in the margins either side of
            the content column and in the gaps between sections, not just behind the hero.
            The nebula color washes live in globals.css (`.dark body`, background-attachment:
            fixed) since they're the same fixed viewport-anchored backdrop everywhere, not
            something that needs to vary per scroll position the way the stars do. */}
        <Starfield seed="page" count={600} />

        <ThemeProvider>
          <a
            href="#main"
            className="sr-only focus:not-sr-only focus:fixed focus:top-4 focus:left-4 focus:z-[100] focus:rounded-full focus:bg-accent focus:px-4 focus:py-2 focus:text-sm focus:font-medium focus:text-accent-fg"
          >
            Skip to content
          </a>
          <NavBar />
          <main id="main" className="relative z-10 flex-1">
            {children}
          </main>
          <Footer />
        </ThemeProvider>
      </body>
    </html>
  );
}
