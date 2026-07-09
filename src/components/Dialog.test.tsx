import { describe, it, expect, vi } from "vitest";
import { readFileSync } from "node:fs";
import { readdirSync } from "node:fs";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import Dialog from "./Dialog";

describe("Dialog", () => {
  it("shows content when open, then hides it and unmounts it when closed", async () => {
    const { rerender } = render(
      <Dialog open={true} onClose={vi.fn()}>
        <p>Hello</p>
      </Dialog>,
    );

    const content = await screen.findByTestId("app-dialog-content");
    expect(content).toBeInTheDocument();
    expect(screen.getByText("Hello")).toBeInTheDocument();

    rerender(
      <Dialog open={false} onClose={vi.fn()}>
        <p>Hello</p>
      </Dialog>,
    );

    await waitFor(() =>
      expect(screen.queryByTestId("app-dialog-content")).not.toBeInTheDocument(),
    );
    expect(screen.queryByText("Hello")).not.toBeInTheDocument();
  });

  it("calls onClose when Escape is pressed", async () => {
    const onClose = vi.fn();
    render(
      <Dialog open={true} onClose={onClose}>
        <p>Hello</p>
      </Dialog>,
    );

    await screen.findByTestId("app-dialog-content");
    await userEvent.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("does not render dialog content while closed", () => {
    render(
      <Dialog open={false} onClose={vi.fn()}>
        <p>Hello</p>
      </Dialog>,
    );

    expect(screen.queryByTestId("app-dialog-content")).not.toBeInTheDocument();
    expect(screen.queryByText("Hello")).not.toBeInTheDocument();
  });

  it("keeps generated ui radii at 8px-or-less across src/components/ui", () => {
    const uiDir = "src/components/ui";
    const roundedXlPattern =
      /\brounded(?:-[trblse]{1,2})?-(?:xl|[2-9]xl)\b|\brounded-[a-z-]*(?:xl|[2-9]xl)\b/;

    for (const entry of readdirSync(uiDir)) {
      if (!entry.endsWith(".tsx") && !entry.endsWith(".ts")) {
        continue;
      }

      const source = readFileSync(`${uiDir}/${entry}`, "utf8");
      expect(source, entry).not.toMatch(roundedXlPattern);
    }
  });
});
