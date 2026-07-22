import type { Metadata } from "next";
import { Geist, Geist_Mono } from "next/font/google";
import { site } from "@/content/site";
import { siteUrl, jsonLd } from "@/lib/seo";
import { MINIMAL_MODE_ATTRIBUTE, MINIMAL_MODE_STORAGE_KEY } from "@/lib/minimalMode";
import { MinimalModeProvider } from "@/components/layout/MinimalModeProvider";
import { ThemeProvider } from "@/components/layout/ThemeProvider";
import { SiteChrome } from "@/components/layout/SiteChrome";
import { MinimalHome } from "@/components/sections/MinimalHome";
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
        {/* Sets the minimal-mode attribute before hydration -- everything else about which
            tree (this full experience or MinimalHome) is visible is decided by CSS keyed off
            of it, so a returning visitor who left in minimal mode never sees a flash of the
            full site first. */}
        <script
          dangerouslySetInnerHTML={{
            __html: `try{if(localStorage.getItem('${MINIMAL_MODE_STORAGE_KEY}')==='true'){document.documentElement.setAttribute('${MINIMAL_MODE_ATTRIBUTE}','true')}}catch(e){}`,
          }}
        />
        <script
          type="application/ld+json"
          dangerouslySetInnerHTML={{ __html: JSON.stringify(jsonLd()) }}
        />

        <MinimalModeProvider>
          <ThemeProvider>
            {/* The page's own starfield, behind every card -- visible in the margins either
                side of the content column and in the gaps between sections, not just behind
                the hero. The nebula color washes live in globals.css (`.dark body`,
                background-attachment: fixed) since they're the same fixed viewport-anchored
                backdrop everywhere, not something that needs to vary per scroll position the
                way the stars do.

                Two layers: a canvas-rendered dust field dense enough to fill the whole document
                height (DOM nodes stop scaling well in the low thousands, so bulk density has to
                be canvas), plus a lighter set of the same hand-tuned accent dots the rest of the
                site uses -- kept as plain DOM so a handful of stars still render with JS
                disabled. The dust field's opacity is tied to LoaderProvider's progress, so it
                fades in alongside the intro's percentage counter instead of just appearing
                instantly. Both are rendered inside SiteChrome, which skips them entirely in
                minimal mode. */}
            <SiteChrome>{children}</SiteChrome>
            <MinimalHome />
          </ThemeProvider>
        </MinimalModeProvider>
      </body>
    </html>
  );
}
