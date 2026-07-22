// Shared between the blocking inline script in RootLayout (plain JS, runs before hydration)
// and MinimalModeProvider (React state) -- both must agree on the same key and attribute name.
export const MINIMAL_MODE_STORAGE_KEY = "hyperion-minimal-mode";
export const MINIMAL_MODE_ATTRIBUTE = "data-minimal";
