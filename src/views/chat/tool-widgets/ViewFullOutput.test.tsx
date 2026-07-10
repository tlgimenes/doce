import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import ViewFullOutput from "./ViewFullOutput";
import { commands } from "@/lib/ipc";
import type { AttachedFile } from "@/lib/ipc";

vi.mock("@/lib/ipc", () => ({
  commands: {
    readAttachedFile: vi.fn(),
  },
}));

describe("ViewFullOutput (010-context-window-management/US3)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("shows a 'View full output' button that fetches and displays the full text on click", async () => {
    // "hello world" base64-encoded, matching read_attached_file's contract
    // (base64, no data: prefix).
    vi.mocked(commands.readAttachedFile).mockResolvedValue({
      data: btoa("hello world"),
      mimeType: "text/plain",
      name: "call1.txt",
    });

    render(<ViewFullOutput path="/data/tool-outputs/conv1/call1.txt" />);
    expect(screen.queryByTestId("view-full-output-content")).not.toBeInTheDocument();

    const button = screen.getByTestId("view-full-output-button");
    expect(button.tagName).toBe("BUTTON");
    expect(button).toHaveTextContent("View full output");

    await userEvent.click(button);

    await waitFor(() =>
      expect(commands.readAttachedFile).toHaveBeenCalledWith("/data/tool-outputs/conv1/call1.txt"),
    );
    const content = await screen.findByTestId("view-full-output-content");
    expect(content).toHaveAttribute("data-slot", "code-block");
    expect(content).toHaveTextContent("hello world");
  });

  it("shows a disabled button with a spinner while the fetch is pending", async () => {
    let resolveFile!: (v: AttachedFile) => void;
    vi.mocked(commands.readAttachedFile).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveFile = resolve;
        }),
    );

    render(<ViewFullOutput path="/data/tool-outputs/conv1/call1.txt" />);
    await userEvent.click(screen.getByTestId("view-full-output-button"));

    const button = screen.getByTestId("view-full-output-button");
    expect(button).toBeDisabled();
    expect(button.querySelector('[data-slot="spinner"]')).not.toBeNull();

    await act(async () => {
      resolveFile({ data: btoa("hello world"), mimeType: "text/plain", name: "call1.txt" });
    });
    expect(await screen.findByTestId("view-full-output-content")).toHaveTextContent("hello world");
  });

  it("shows an error message if the underlying file can't be read", async () => {
    vi.mocked(commands.readAttachedFile).mockRejectedValue(new Error("file not found"));

    render(<ViewFullOutput path="/data/tool-outputs/conv1/missing.txt" />);
    await userEvent.click(screen.getByTestId("view-full-output-button"));

    expect(await screen.findByText(/file not found/)).toBeInTheDocument();
  });
});
