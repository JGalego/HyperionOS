export const gettingStarted = {
  kicker: "Getting started",
  heading: "You can read every line of it.",
  body: "Grab a signed, ready-to-flash release image, or build every layer yourself from source.",
  tabs: [
    {
      id: "release",
      label: "From a release",
      steps: [
        {
          title: "Download the image",
          detail: "Every tagged release publishes a ready-to-flash x86_64 disk image on GitHub Releases.",
          code: "# https://github.com/JGalego/HyperionOS/releases/latest\nhyperion-x86_64-<version>.img",
        },
        {
          title: "Flash it with balenaEtcher",
          detail:
            "Open Etcher, select the downloaded .img and your USB drive, and flash. Etcher writes the raw image directly and verifies the write afterward.",
          code: undefined,
        },
        {
          title: "Verify the signature (optional, recommended)",
          detail: "Every image ships with an Ed25519-signed manifest you can check independently before you trust it.",
          code: "cargo run --release -p hyperion-release-gate --bin verify-release -- \\\n  hyperion-x86_64-<version>.img hyperion-x86_64-<version>.img.release.json",
        },
        {
          title: "Boot it",
          detail: "Boot the target machine from the USB drive (usually a one-time boot-menu key like F12/F10/Esc) and select it.",
          code: undefined,
        },
      ],
    },
    {
      id: "source",
      label: "From source",
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
    },
  ],
} as const;
