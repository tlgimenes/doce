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

  it("includes workbench previews for buttons, command center, and settings rows", () => {
    render(<WidgetGallery onClose={vi.fn()} />);

    expect(screen.getByText("Button variants")).toBeInTheDocument();
    expect(screen.getByText("Button sizes")).toBeInTheDocument();
    expect(screen.getByText("Command center preview")).toBeInTheDocument();
    expect(screen.getByText("Settings row preview")).toBeInTheDocument();
  });

  it("enumerates the stock button variants and sizes", () => {
    render(<WidgetGallery onClose={vi.fn()} />);

    expect(screen.getByRole("button", { name: "Default" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Outline" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Secondary" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Ghost" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Destructive" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Link" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Primary" })).not.toBeInTheDocument();

    expect(screen.getByRole("button", { name: "Icon extra small" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Icon small" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Icon default" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Icon large" })).toBeInTheDocument();
  });

  it("documents the standard theme tokens as color swatches", () => {
    render(<WidgetGallery onClose={vi.fn()} />);

    expect(screen.getByText("Theme tokens")).toBeInTheDocument();
    expect(screen.getByText("Color tokens")).toBeInTheDocument();
    expect(screen.getByText("--background")).toBeInTheDocument();
    expect(screen.getByText("--primary")).toBeInTheDocument();
    expect(screen.getByText("--destructive")).toBeInTheDocument();
    expect(screen.getByText("--chart-1")).toBeInTheDocument();
    expect(screen.getByText("--chart-5")).toBeInTheDocument();
    expect(screen.queryByText("--color-doce-caramel")).not.toBeInTheDocument();
    expect(screen.queryByText("Brand Accent Workbench")).not.toBeInTheDocument();
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
