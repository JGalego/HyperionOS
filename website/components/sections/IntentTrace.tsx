"use client";

import { motion } from "framer-motion";
import type { traces } from "@/content/traces";
import { useReducedMotionSafe } from "@/components/motion/useReducedMotionSafe";

type Trace = (typeof traces)[number];

export function IntentTrace({ trace }: { trace: Trace }) {
  const reduceMotion = useReducedMotionSafe();

  const cardVariants = reduceMotion
    ? { hidden: { opacity: 1, y: 0 }, visible: { opacity: 1, y: 0 } }
    : { hidden: { opacity: 0, y: 16 }, visible: { opacity: 1, y: 0 } };

  const chipVariants = reduceMotion
    ? { hidden: { opacity: 1, scale: 1 }, visible: { opacity: 1, scale: 1 } }
    : { hidden: { opacity: 0, scale: 0.9 }, visible: { opacity: 1, scale: 1 } };

  return (
    <motion.div
      className="rounded-2xl border border-border bg-bg-elevated p-6 md:p-8"
      initial="hidden"
      whileInView="visible"
      viewport={{ once: true, margin: "-80px" }}
      variants={cardVariants}
      transition={{ duration: 0.5 }}
    >
      <p className="font-mono text-sm text-accent-strong">&ldquo;{trace.prompt}&rdquo;</p>
      <p className="mt-3 text-sm text-fg-muted">{trace.context}</p>

      <motion.ul
        className="mt-6 flex flex-wrap gap-2"
        initial="hidden"
        whileInView="visible"
        viewport={{ once: true, margin: "-80px" }}
        variants={{ visible: { transition: { staggerChildren: reduceMotion ? 0 : 0.08 } } }}
      >
        {trace.assembled.map((item) => (
          <motion.li
            key={item}
            className="rounded-full bg-accent/10 px-3 py-1.5 text-sm text-fg"
            variants={chipVariants}
          >
            {item}
          </motion.li>
        ))}
      </motion.ul>
    </motion.div>
  );
}
