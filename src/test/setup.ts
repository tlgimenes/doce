import "@testing-library/jest-dom/vitest";
import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";

// Not automatic under vitest by default — without this, each test's render
// output accumulates in the DOM instead of being torn down, which broke
// exactly this way: a second test's `findByTestId` failed because the
// previous test's still-mounted component had a matching element too.
afterEach(() => {
  cleanup();
});
