import { describe, it, expect, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import Dialog from "./Dialog";

describe("Dialog", () => {
  it("shows content when open, then hides it and unmounts it when closed", async () => {
    const { rerender } = render(
      <Dialog open={true} onClose={vi.fn()} title="Test dialog">
        <p>Hello</p>
      </Dialog>,
    );

    const content = await screen.findByTestId("app-dialog-content");
    expect(content).toBeInTheDocument();
    expect(screen.getByText("Hello")).toBeInTheDocument();

    rerender(
      <Dialog open={false} onClose={vi.fn()} title="Test dialog">
        <p>Hello</p>
      </Dialog>,
    );

    await waitFor(() => expect(screen.queryByTestId("app-dialog-content")).not.toBeInTheDocument());
    expect(screen.queryByText("Hello")).not.toBeInTheDocument();
  });

  it("calls onClose when Escape is pressed", async () => {
    const onClose = vi.fn();
    render(
      <Dialog open={true} onClose={onClose} title="Test dialog">
        <p>Hello</p>
      </Dialog>,
    );

    await screen.findByTestId("app-dialog-content");
    await userEvent.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("uses the provided title and description as the dialog's accessible name and description", async () => {
    render(
      <Dialog
        open={true}
        onClose={vi.fn()}
        title="Search conversations"
        description="Find a conversation by title or message content."
      >
        <p>Hello</p>
      </Dialog>,
    );

    const dialog = await screen.findByRole("dialog", { name: "Search conversations" });
    expect(dialog).toHaveAccessibleDescription("Find a conversation by title or message content.");
  });

  it("applies a content class override to the dialog shell", async () => {
    render(
      <Dialog open={true} onClose={vi.fn()} title="Command center" contentClassName="w-[34rem]">
        <p>Hello</p>
      </Dialog>,
    );

    expect(await screen.findByTestId("app-dialog-content")).toHaveClass("w-[34rem]");
  });

  it("does not render dialog content while closed", () => {
    render(
      <Dialog open={false} onClose={vi.fn()} title="Test dialog">
        <p>Hello</p>
      </Dialog>,
    );

    expect(screen.queryByTestId("app-dialog-content")).not.toBeInTheDocument();
    expect(screen.queryByText("Hello")).not.toBeInTheDocument();
  });
});
