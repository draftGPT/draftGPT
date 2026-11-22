import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vitest/config";

// Unit and component tests of the shared library (draft-ui/tests/*.test.ts).
export default defineConfig({
  plugins: [svelte()],
  resolve: { conditions: ["browser"] },
  test: {
    environment: "jsdom",
    // Testing Library unmounts between tests through the global afterEach.
    globals: true,
    include: ["draft-ui/tests/**/*.test.ts"],
  },
});
