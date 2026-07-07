import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useEffect } from "react";
import { useEditor, EditorContent, type Editor } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import SkillMention from "./skill-mention";
import { commands } from "@/lib/ipc";

/**
 * 009-rich-chat-input, User Story 3 (T030): the "/" skill-mention picker
 * (spec.md's US3 acceptance scenarios; research.md's "Skill-mention popup"
 * decision). Tier-2 jsdom component test per research.md's Testing
 * strategy — a minimal editor mounting *only* this extension (plus the
 * base document schema), structural assertions only, no pixel geometry
 * (research.md's "Resolved (T003 spike)" note is explicit that sequential
 * arrow-key navigation isn't reliably testable here — that's T032's
 * concern, targeting WDIO e2e instead; this file only covers open/list/
 * filter/click-select/Escape/empty-state).
 */
vi.mock("@/lib/ipc", () => ({
  commands: {
    listSkills: vi.fn(),
  },
}));

function TestHarness({ onReady }: { onReady: (editor: Editor) => void }) {
  const editor = useEditor({
    extensions: [
      StarterKit.configure({
        heading: false,
        blockquote: false,
        codeBlock: false,
        horizontalRule: false,
        dropcursor: false,
      }),
      SkillMention,
    ],
    editorProps: {
      attributes: { "data-testid": "test-editor" },
    },
  });

  useEffect(() => {
    if (editor) onReady(editor);
  }, [editor, onReady]);

  return <EditorContent editor={editor} />;
}

/** Renders the harness and resolves once the editor instance is available. */
async function setup(): Promise<Editor> {
  let current: Editor | null = null;
  render(<TestHarness onReady={(editor) => (current = editor)} />);
  await waitFor(() => expect(current).not.toBeNull());
  return current!;
}

const SKILLS = [
  { name: "reviewer", description: "Reviews code changes" },
  { name: "planner", description: "Plans your day" },
];

describe("skillMention extension (009-rich-chat-input, US3)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('typing "/" opens a picker listing the fetched skills by name and description', async () => {
    vi.mocked(commands.listSkills).mockResolvedValue(SKILLS);
    const editor = await setup();
    const editable = screen.getByTestId("test-editor");
    const user = userEvent.setup();

    await user.click(editable);
    await user.type(editable, "/");

    await screen.findByTestId("skill-mention-popup");
    expect(screen.getByText("reviewer")).toBeInTheDocument();
    expect(screen.getByText("Reviews code changes")).toBeInTheDocument();
    expect(screen.getByText("planner")).toBeInTheDocument();
    expect(screen.getByText("Plans your day")).toBeInTheDocument();
    expect(editor.getText()).toBe("/");
  });

  it('typing further characters after "/" filters the list client-side by name or description, case-insensitively', async () => {
    vi.mocked(commands.listSkills).mockResolvedValue(SKILLS);
    const editor = await setup();
    const editable = screen.getByTestId("test-editor");
    const user = userEvent.setup();

    await user.click(editable);
    await user.type(editable, "/");
    await screen.findByTestId("skill-mention-popup");

    // Filter by name.
    await user.type(editable, "PLAN");
    await waitFor(() => {
      expect(screen.getByText("planner")).toBeInTheDocument();
      expect(screen.queryByText("reviewer")).not.toBeInTheDocument();
    });

    // Reset to an empty doc and filter by description instead (avoids
    // depending on real-keystroke backspace-through-a-decoration behavior,
    // which research.md's Testing strategy already flags as a tier-2
    // rough edge near ProseMirror decorations/NodeViews).
    act(() => {
      editor.commands.clearContent(true);
    });
    await user.click(editable);
    await user.type(editable, "/code");
    await waitFor(() => {
      expect(screen.getByText("reviewer")).toBeInTheDocument();
      expect(screen.queryByText("planner")).not.toBeInTheDocument();
    });
  });

  it("selecting an item via click inserts a skillMention node at the trigger's range and closes the picker", async () => {
    vi.mocked(commands.listSkills).mockResolvedValue(SKILLS);
    const editor = await setup();
    const editable = screen.getByTestId("test-editor");
    const user = userEvent.setup();

    await user.click(editable);
    await user.type(editable, "/rev");
    await screen.findByTestId("skill-mention-popup");

    const items = await screen.findAllByTestId("skill-mention-item");
    const reviewerItem = items.find((item) => item.textContent?.includes("reviewer"));
    expect(reviewerItem).toBeDefined();
    await user.click(reviewerItem!);

    await waitFor(() => {
      expect(screen.queryByTestId("skill-mention-popup")).not.toBeInTheDocument();
    });

    // The typed "/rev" query text is gone, replaced by the chip — not left
    // over as plain text alongside it.
    expect(editor.getText()).not.toContain("/rev");

    const doc = editor.getJSON();
    const flatNodes = JSON.stringify(doc);
    expect(flatNodes).toContain('"type":"skillMention"');

    const chip = await screen.findByTestId("skill-mention-chip");
    expect(chip).toHaveTextContent("/reviewer");
  });

  it("pressing Escape closes the picker without inserting anything, leaving the typed text as plain text", async () => {
    vi.mocked(commands.listSkills).mockResolvedValue(SKILLS);
    const editor = await setup();
    const editable = screen.getByTestId("test-editor");
    const user = userEvent.setup();

    await user.click(editable);
    await user.type(editable, "/foo");
    await screen.findByTestId("skill-mention-popup");

    await user.keyboard("{Escape}");

    await waitFor(() => {
      expect(screen.queryByTestId("skill-mention-popup")).not.toBeInTheDocument();
    });
    expect(editor.getText()).toBe("/foo");
    expect(screen.queryByTestId("skill-mention-chip")).not.toBeInTheDocument();
  });

  // --- 010-context-window-management (UI refactor): `/compact` is a
  // reserved slash command, not a skill mention — it must not keep the
  // picker active once fully typed (which would swallow the Enter that's
  // supposed to submit it).

  it('typing "/compact" exactly closes the picker (falls back to plain Enter-to-submit behavior)', async () => {
    vi.mocked(commands.listSkills).mockResolvedValue(SKILLS);
    const editor = await setup();
    const editable = screen.getByTestId("test-editor");
    const user = userEvent.setup();

    await user.click(editable);
    await user.type(editable, "/compac");
    // Still mid-typing the reserved word — the picker behaves normally
    // (open, filtering, "no matching skills" for this query).
    await screen.findByTestId("skill-mention-popup");

    await user.type(editable, "t");
    await waitFor(() => {
      expect(screen.queryByTestId("skill-mention-popup")).not.toBeInTheDocument();
    });
    expect(editor.getText()).toBe("/compact");
  });

  it('typing "/compact" from scratch never opens the picker at all once complete', async () => {
    vi.mocked(commands.listSkills).mockResolvedValue(SKILLS);
    await setup();
    const editable = screen.getByTestId("test-editor");
    const user = userEvent.setup();

    await user.click(editable);
    await user.type(editable, "/compact");

    // No intermediate render ever leaves the popup mounted once the full
    // word is in place.
    expect(screen.queryByTestId("skill-mention-popup")).not.toBeInTheDocument();
  });

  it("shows a legible empty state instead of a blank popup when no skills are installed (FR-015)", async () => {
    vi.mocked(commands.listSkills).mockResolvedValue([]);
    await setup();
    const editable = screen.getByTestId("test-editor");
    const user = userEvent.setup();

    await user.click(editable);
    await user.type(editable, "/");

    await screen.findByTestId("skill-mention-popup");
    const emptyState = await screen.findByTestId("skill-mention-empty");
    expect(emptyState).toBeInTheDocument();
    expect(emptyState.textContent).toMatch(/no skills/i);
    expect(screen.queryByTestId("skill-mention-item")).not.toBeInTheDocument();
  });
});
