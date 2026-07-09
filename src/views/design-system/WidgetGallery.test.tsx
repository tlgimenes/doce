import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import WidgetGallery from "./WidgetGallery";

describe("WidgetGallery", () => {
  it("fills the shell content area instead of forcing viewport height", () => {
    render(<WidgetGallery onClose={vi.fn()} />);

    expect(screen.getByTestId("widget-gallery")).toHaveClass("h-full");
    expect(screen.getByTestId("widget-gallery")).not.toHaveClass("h-dvh");
  });

  it("documents Read as collapsed expandable previews", () => {
    render(<WidgetGallery onClose={vi.fn()} />);

    expect(
      screen.getByText("A collapsed file-reference card with inline expandable preview."),
    ).toBeInTheDocument();
    expect(screen.getByText("Text read")).toBeInTheDocument();
    expect(screen.getByText("Native preview candidate")).toBeInTheDocument();
    expect(screen.queryByText("Offloaded read")).not.toBeInTheDocument();
    expect(screen.queryByText("Truncated")).not.toBeInTheDocument();
  });

  it("documents search widgets as collapsed expandable result lists", () => {
    render(<WidgetGallery onClose={vi.fn()} />);

    expect(
      screen.getByText("Collapsed search summaries with inline expandable result lists."),
    ).toBeInTheDocument();
    expect(screen.getByText("Glob, with files")).toBeInTheDocument();
    expect(screen.getByText("Glob, no files")).toBeInTheDocument();
    expect(screen.getByText("Grep, with matches")).toBeInTheDocument();
    expect(screen.getByText("Grep, no matches")).toBeInTheDocument();
  });
});
