import { describe, it, expect, vi } from "vitest";
import { readFileSync } from "node:fs";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import Dialog from "./Dialog";

describe("Dialog", () => {
  it("shows the dialog when open, then hides it and unmounts its content", async () => {
    const { container, rerender } = render(
      <Dialog open={true} onClose={vi.fn()}>
        <p>Hello</p>
      </Dialog>,
    );
    const dialog = container.querySelector("dialog") as HTMLDialogElement;

    await waitFor(() => expect(dialog.open).toBe(true));
    expect(screen.getByTestId("app-dialog-content")).toBeInTheDocument();
    expect(screen.getByText("Hello")).toBeInTheDocument();

    rerender(
      <Dialog open={false} onClose={vi.fn()}>
        <p>Hello</p>
      </Dialog>,
    );

    await waitFor(() => expect(dialog.open).toBe(false));
    expect(screen.queryByText("Hello")).not.toBeInTheDocument();
  });

  it("calls onClose when the native cancel event fires", async () => {
    const onClose = vi.fn();
    render(
      <Dialog open={true} onClose={onClose}>
        <p>Hello</p>
      </Dialog>,
    );

    const dialog = await screen
      .findByText("Hello")
      .then((element) => element.closest("dialog") as HTMLDialogElement);

    dialog.dispatchEvent(new Event("cancel", { cancelable: true }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("calls onClose on a backdrop click, but not on content clicks", async () => {
    const onClose = vi.fn();
    const { container } = render(
      <Dialog open={true} onClose={onClose}>
        <p data-testid="content">Hello</p>
      </Dialog>,
    );

    await userEvent.click(screen.getByTestId("content"));
    expect(onClose).not.toHaveBeenCalled();

    const dialog = container.querySelector("dialog") as HTMLDialogElement;
    await userEvent.click(dialog);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("keeps the generated dialog foundation at 8px-or-less radii", () => {
    const source = readFileSync("src/components/ui/dialog.tsx", "utf8");

    expect(source).not.toContain("rounded-xl");
    expect(source).not.toContain("rounded-b-xl");
    expect(source).toContain("rounded-lg");
  });
});
