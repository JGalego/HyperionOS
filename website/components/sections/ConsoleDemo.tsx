"use client";

import { motion } from "framer-motion";
import { ArrowUpRight } from "lucide-react";
import type { consoleDemos } from "@/content/console-demos";
import { site } from "@/content/site";
import { useReducedMotionSafe } from "@/components/motion/useReducedMotionSafe";

type Demo = (typeof consoleDemos)[number];

/**
 * Renders a real hyperion-console transcript as real, selectable text -- not a screen recording.
 * Deliberately no video/GIF asset: a screen reader can read every line, the text can be copied,
 * and it costs nothing to load, matching CLAUDE.md's accessibility-first stance. The README's own
 * assets/demo-*.gif recordings of these same scenarios exist for GitHub, which can't run this
 * component's JS -- this is the web-native equivalent, not a duplicate for its own sake.
 */
export function ConsoleDemo({ demo }: { demo: Demo }) {
  const reduceMotion = useReducedMotionSafe();

  const cardVariants = reduceMotion
    ? { hidden: { opacity: 1, y: 0 }, visible: { opacity: 1, y: 0 } }
    : { hidden: { opacity: 0, y: 16 }, visible: { opacity: 1, y: 0 } };

  const lineVariants = reduceMotion
    ? { hidden: { opacity: 1 }, visible: { opacity: 1 } }
    : { hidden: { opacity: 0 }, visible: { opacity: 1 } };

  return (
    <motion.div
      className="rounded-2xl border border-border bg-bg-elevated p-6 md:p-8"
      initial="hidden"
      whileInView="visible"
      viewport={{ once: true, margin: "-80px" }}
      variants={cardVariants}
      transition={{ duration: 0.5 }}
    >
      <p className="font-medium">{demo.title}</p>
      <p className="mt-1 text-sm text-fg-muted">{demo.description}</p>

      <motion.pre
        className="mt-5 overflow-x-auto whitespace-pre-wrap break-words rounded-xl bg-bg-muted p-4 font-mono text-[13px] leading-relaxed"
        initial="hidden"
        whileInView="visible"
        viewport={{ once: true, margin: "-80px" }}
        variants={{ visible: { transition: { staggerChildren: reduceMotion ? 0 : 0.12 } } }}
      >
        {demo.lines.map((line, index) => (
          <motion.span key={index} className="block" variants={lineVariants}>
            {line.type === "input" ? (
              <>
                <span className="text-accent-strong">{"> "}</span>
                <span className="text-fg">{line.text}</span>
              </>
            ) : (
              <span className="text-fg-muted">{line.text}</span>
            )}
          </motion.span>
        ))}
      </motion.pre>

      <a
        href={`${site.githubUrl}/blob/main/${demo.scenarioFile}`}
        target="_blank"
        rel="noopener noreferrer"
        className="group mt-4 inline-flex items-center gap-1 text-sm text-fg-muted transition-colors hover:text-accent-strong"
      >
        {demo.scenarioFile}
        <ArrowUpRight
          className="h-3.5 w-3.5 transition-transform group-hover:translate-x-0.5 group-hover:-translate-y-0.5"
          aria-hidden="true"
        />
      </a>
    </motion.div>
  );
}
