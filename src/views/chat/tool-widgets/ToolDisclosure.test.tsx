import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import ToolDisclosure from "./ToolDisclosure";

describe("ToolDisclosure", () => {
  it("renders collapsed by default and expands inline when the summary is clicked", async () => {
    render(
      <ToolDisclosure
        summary={<span>Read src/App.tsx · 120B</span>}
        testId="tool-disclosure"
        summaryTestId="tool-summary"
        bodyTestId="tool-body"
      >
        <p>expanded preview</p>
      </ToolDisclosure>,
    );

    const disclosure = screen.getByTestId("tool-disclosure");
    expect(disclosure).not.toHaveAttribute("open");
    expect(screen.getByTestId("tool-summary")).toHaveTextContent("Read src/App.tsx");
    expect(screen.queryByTestId("tool-body")).not.toBeInTheDocument();

    await userEvent.click(screen.getByTestId("tool-summary"));

    expect(disclosure).toHaveAttribute("open");
    expect(screen.getByTestId("tool-body")).toHaveTextContent("expanded preview");
  });

  it("renders a right-side decorative chevron and height-limited body", async () => {
    render(
      <ToolDisclosure
        summary={<span>Glob *.tsx · 3 files</span>}
        testId="tool-disclosure"
        summaryTestId="tool-summary"
        bodyTestId="tool-body"
      >
        <p>file list</p>
      </ToolDisclosure>,
    );

    await userEvent.click(screen.getByTestId("tool-summary"));

    expect(screen.getByTestId("tool-disclosure-chevron")).toHaveAttribute("aria-hidden", "true");
    expect(screen.getByTestId("tool-body")).toHaveClass("max-h-80");
    expect(screen.getByTestId("tool-body")).toHaveClass("overflow-y-auto");
  });

  it("uses the same compact header rhythm as the edit diff header", () => {
    render(
      <ToolDisclosure
        summary={<span>Read /tmp/notes.txt · 11B</span>}
        testId="tool-disclosure"
        summaryTestId="tool-summary"
        bodyTestId="tool-body"
      >
        <p>expanded preview</p>
      </ToolDisclosure>,
    );

    expect(screen.getByTestId("tool-disclosure")).toHaveClass("overflow-hidden");
    expect(screen.getByTestId("tool-summary")).toHaveClass("px-3", "py-1.5");
    expect(screen.getByTestId("tool-summary")).not.toHaveClass("py-2");
  });
});
