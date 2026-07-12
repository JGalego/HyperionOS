export const gettingStarted = {
  kicker: "Getting started",
  heading: "You can read every line of it.",
  body: "There's no installer yet, because we'd rather ship something real than something that merely looks finished.",
  steps: [
    {
      title: "Clone the repository",
      detail: "Everything, kernel through experience layer, is public from day one.",
      code: "git clone https://github.com/JGalego/HyperionOS",
    },
    {
      title: "Boot the reference image",
      detail: "Build and boot a real image under QEMU using the scripts in boot/scripts.",
      code: "./boot/scripts/build-image-aarch64.sh",
    },
    {
      title: "Follow along",
      detail: "Track progress, open issues, and see what's landing next on GitHub.",
      code: undefined,
    },
  ],
} as const;
