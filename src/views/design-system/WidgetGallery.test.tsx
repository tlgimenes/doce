import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import WidgetGallery from "./WidgetGallery";

describe("WidgetGallery", () => {
  it("fills the shell content area instead of forcing viewport height", () => {
    render(<WidgetGallery onClose={vi.fn()} />);

    expect(screen.getByTestId("widget-gallery")).toHaveClass("h-full");
    expect(screen.getByTestId("widget-gallery")).not.toHaveClass("h-dvh");
  });

  it("documents Read as grouped successful reads rather than separate truncated state", () => {
    render(<WidgetGallery onClose={vi.fn()} />);

    expect(
      screen.getByText("A minimal file-reference card. Standard / offloaded / failure."),
    ).toBeInTheDocument();
    expect(screen.getByText("Standard read")).toBeInTheDocument();
    expect(screen.getByText("Offloaded read")).toBeInTheDocument();
    expect(screen.queryByText("Truncated")).not.toBeInTheDocument();
    expect(screen.queryByText("Offloaded (large file)")).not.toBeInTheDocument();
    expect(
      screen.queryByText(
        "A file-reference card, not a raw content dump. Success / truncated / offloaded / failure.",
      ),
    ).not.toBeInTheDocument();
  });
});
