import { describe, it, expect, vi, beforeEach } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { TopbarHost, TopbarPortal, TopbarProvider } from "./Topbar";

const startDragging = vi.fn();

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    startDragging,
  }),
}));

describe("Topbar", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    startDragging.mockResolvedValue(undefined);
  });

  it("renders fixed-height draggable hosts for sidebar and main", () => {
    render(
      <TopbarProvider>
        <TopbarHost target="sidebar" />
        <TopbarHost target="main" />
      </TopbarProvider>,
    );

    const sidebar = screen.getByTestId("topbar-sidebar");
    const main = screen.getByTestId("topbar-main");

    expect(sidebar).toHaveClass(
      "flex",
      "h-10",
      "shrink-0",
      "select-none",
      "items-center",
      "bg-transparent",
      "text-foreground",
    );
    expect(main).toHaveClass(
      "flex",
      "h-10",
      "shrink-0",
      "select-none",
      "items-center",
      "bg-transparent",
      "text-foreground",
    );
    expect(sidebar).toHaveAttribute("data-tauri-drag-region");
    expect(main).toHaveAttribute("data-tauri-drag-region");
  });

  it("portals children into the matching host", async () => {
    render(
      <TopbarProvider>
        <TopbarHost target="main" />
        <TopbarPortal target="main">
          <div data-testid="main-topbar-content">Thread title</div>
        </TopbarPortal>
      </TopbarProvider>,
    );

    const host = screen.getByTestId("topbar-main");
    expect(await screen.findByTestId("main-topbar-content")).toBeInTheDocument();
    expect(host).toHaveTextContent("Thread title");
  });

  it("starts dragging only for the primary mouse button", () => {
    render(
      <TopbarProvider>
        <TopbarHost target="main" />
      </TopbarProvider>,
    );

    const host = screen.getByTestId("topbar-main");
    fireEvent.mouseDown(host, { button: 2 });
    expect(startDragging).not.toHaveBeenCalled();

    fireEvent.mouseDown(host, { button: 0 });
    expect(startDragging).toHaveBeenCalledTimes(1);
  });

  it("does not start dragging from children marked as non-drag controls", () => {
    render(
      <TopbarProvider>
        <TopbarHost target="main">
          <button type="button" data-topbar-no-drag>
            Control
          </button>
        </TopbarHost>
      </TopbarProvider>,
    );

    fireEvent.mouseDown(screen.getByRole("button", { name: "Control" }), { button: 0 });
    expect(startDragging).not.toHaveBeenCalled();
  });
});
