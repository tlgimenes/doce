import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import App from "./App";
import "./styles/theme.css";

// Only loaded when built for e2e testing (tests/e2e/build-for-e2e.sh sets
// VITE_E2E_TESTING) — the wdio execute/mock bridge has no reason to ship in
// a normal dev or release build.
if (import.meta.env.VITE_E2E_TESTING === "true") {
  await import("@wdio/tauri-plugin");
}

const queryClient = new QueryClient();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>
  </StrictMode>,
);
