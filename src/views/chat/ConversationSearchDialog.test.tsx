import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import ConversationSearchDialog from "./ConversationSearchDialog";

describe("ConversationSearchDialog", () => {
  it("renders SearchPanel in a dialog and closes on Escape", async () => {
    const onOpenChange = vi.fn();
    render(
      <ConversationSearchDialog
        open={true}
        onOpenChange={onOpenChange}
        recentConversations={[]}
        onSelectConversationId={vi.fn()}
      />,
    );

    expect(screen.getByTestId("conversation-search-dialog")).toBeInTheDocument();
    expect(screen.getByTestId("search-panel")).toBeInTheDocument();

    await userEvent.keyboard("{Escape}");
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});
