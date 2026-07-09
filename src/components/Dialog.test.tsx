import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import Dialog from "./Dialog";

describe("Dialog", () => {
  it("renders dialog content only while open", () => {
    const { rerender } = render(
      <Dialog open={true} onClose={vi.fn()}>
        <p>Hello</p>
      </Dialog>,
    );

    expect(screen.getByTestId("app-dialog-content")).toBeInTheDocument();
    expect(screen.getByText("Hello")).toBeInTheDocument();

    rerender(
      <Dialog open={false} onClose={vi.fn()}>
        <p>Hello</p>
      </Dialog>,
    );

    expect(screen.queryByText("Hello")).not.toBeInTheDocument();
  });

  it("calls onClose when Escape closes the dialog", async () => {
    const onClose = vi.fn();
    render(
      <Dialog open={true} onClose={onClose}>
        <p>Hello</p>
      </Dialog>,
    );

    await userEvent.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
