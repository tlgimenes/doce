import { describe, it, expect, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import Dialog from "./Dialog";

describe("Dialog", () => {
  it("shows the dialog modally when open, and hides it (and unmounts its content) when open becomes false", async () => {
    const { container, rerender } = render(
      <Dialog open={true} onClose={vi.fn()}>
        <p>Hello</p>
      </Dialog>,
    );
    const dialog = container.querySelector("dialog") as HTMLDialogElement;

    await waitFor(() => expect(dialog.open).toBe(true));
    expect(screen.getByText("Hello")).toBeInTheDocument();

    rerender(
      <Dialog open={false} onClose={vi.fn()}>
        <p>Hello</p>
      </Dialog>,
    );

    await waitFor(() => expect(dialog.open).toBe(false));
    expect(screen.queryByText("Hello")).not.toBeInTheDocument();
  });

  it("calls onClose when the native cancel event fires (Escape, per the browser's own <dialog> behavior)", async () => {
    const onClose = vi.fn();
    render(
      <Dialog open={true} onClose={onClose}>
        <p>Hello</p>
      </Dialog>,
    );

    const dialog = await screen.findByText("Hello").then((el) => el.closest("dialog") as HTMLDialogElement);
    dialog.dispatchEvent(new Event("cancel", { cancelable: true }));

    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("calls onClose on a click landing on the backdrop, but not on a click inside the content", async () => {
    const onClose = vi.fn();
    render(
      <Dialog open={true} onClose={onClose}>
        <p data-testid="content">Hello</p>
      </Dialog>,
    );

    await userEvent.click(screen.getByTestId("content"));
    expect(onClose).not.toHaveBeenCalled();

    const dialog = screen.getByTestId("content").closest("dialog") as HTMLDialogElement;
    await userEvent.click(dialog);
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
