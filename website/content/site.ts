export const site = {
  name: "Hyperion",
  tagline: "The first intent-native operating system.",
  description:
    "Hyperion is an intent-native operating system: humans express goals, and the system determines how those goals become reality.",
  githubUrl: "https://github.com/JGalego/HyperionOS",
  githubDocsUrl: "https://github.com/JGalego/HyperionOS/tree/main/docs",
  license: "MIT",
} as const;

export function githubDoc(path: string) {
  return `${site.githubUrl}/blob/main/docs/${path}`;
}
