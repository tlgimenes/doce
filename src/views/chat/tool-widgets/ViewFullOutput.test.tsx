import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import ViewFullOutput from "./ViewFullOutput";
import { commands } from "@/lib/ipc";

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

    await userEvent.click(screen.getByTestId("view-full-output-button"));

    await waitFor(() =>
      expect(commands.readAttachedFile).toHaveBeenCalledWith("/data/tool-outputs/conv1/call1.txt"),
    );
    expect(await screen.findByTestId("view-full-output-content")).toHaveTextContent("hello world");
  });

  it("shows an error message if the underlying file can't be read", async () => {
    vi.mocked(commands.readAttachedFile).mockRejectedValue(new Error("file not found"));

    render(<ViewFullOutput path="/data/tool-outputs/conv1/missing.txt" />);
    await userEvent.click(screen.getByTestId("view-full-output-button"));

    expect(await screen.findByText(/file not found/)).toBeInTheDocument();
  });
});
