import { Hero } from "@/components/sections/Hero";
import { Philosophy } from "@/components/sections/Philosophy";
import { WhyHyperion } from "@/components/sections/WhyHyperion";
import { Architecture } from "@/components/sections/Architecture";
import { HowItWorks } from "@/components/sections/HowItWorks";
import { LiveConsole } from "@/components/sections/LiveConsole";
import { Features } from "@/components/sections/Features";
import { GettingStarted } from "@/components/sections/GettingStarted";
import { Documentation } from "@/components/sections/Documentation";
import { OpenSource } from "@/components/sections/OpenSource";

export default function Home() {
  return (
    <>
      <Hero />
      <Philosophy />
      <WhyHyperion />
      <Architecture />
      <HowItWorks />
      <LiveConsole />
      <Features />
      <GettingStarted />
      <Documentation />
      <OpenSource />
    </>
  );
}
