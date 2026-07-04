import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import RichInput from "./RichInput";
import { commands } from "@/lib/ipc";

/**
 * 009-rich-chat-input, User Story 3 (T033): confirms `RichInput`'s
 * `skillsEnabled` prop actually gates the "/" skill-mention picker
 * end-to-end (FR-011), and (T024 completion) that a selected skill's
 * segment really flows through `RichInput`'s own `submitCurrentContent`
 * path into `onSubmit`'s `richContent` argument — not just that
 * `serialize.ts`'s pure functions produce the right shape in isolation
 * (`serialize.test.ts`), or that `skill-mention.tsx`'s own extension
 * inserts the right node in isolation (`skill-mention.test.tsx`).
 *
 * Split into its own file (rather than added to `RichInput.test.tsx`)
 * purely so its `vi.mock("@/lib/ipc")` — needed because the `SkillMention`
 * extension calls `commands.listSkills()` — stays scoped to the tests that
 * actually need it.
 */
vi.mock("@/lib/ipc", () => ({
  commands: {
    listSkills: vi.fn(),
  },
}));

const SKILLS = [
  { name: "reviewer", description: "Reviews code changes" },
  { name: "planner", description: "Plans your day" },
];

describe("RichInput (009-rich-chat-input, US3 — skillsEnabled gating)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('skillsEnabled=true: typing "/" calls commands.listSkills() and opens the picker', async () => {
    vi.mocked(commands.listSkills).mockResolvedValue(SKILLS);
    render(
      <RichInput
        onSubmit={vi.fn()}
        skillsEnabled={true}
        disabled={false}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    const editable = screen.getByTestId("test-input");
    const user = userEvent.setup();
    await user.click(editable);
    await user.type(editable, "/");

    await screen.findByTestId("skill-mention-popup");
    expect(commands.listSkills).toHaveBeenCalledTimes(1);
    expect(screen.getByText("reviewer")).toBeInTheDocument();
  });

  it('skillsEnabled=false: typing "/" never calls commands.listSkills() and no picker appears (FR-011)', async () => {
    vi.mocked(commands.listSkills).mockResolvedValue(SKILLS);
    render(
      <RichInput
        onSubmit={vi.fn()}
        skillsEnabled={false}
        disabled={false}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    const editable = screen.getByTestId("test-input");
    const user = userEvent.setup();
    await user.click(editable);
    await user.type(editable, "/reviewer some text");

    // "/" is fully inert here — no suggestion plugin was ever registered
    // (RichInput.tsx conditionally omits the `SkillMention` extension
    // entirely when `skillsEnabled` is false), so there is no async gap to
    // wait out: nothing could have scheduled a `listSkills()` call at all.
    expect(commands.listSkills).not.toHaveBeenCalled();
    expect(screen.queryByTestId("skill-mention-popup")).not.toBeInTheDocument();
    // The "/" is left in the document as ordinary typed text.
    expect(editable).toHaveTextContent("/reviewer some text");
  });

  it("selecting a skill and pressing Enter confirms the picker selection (not a premature submit), and a later Enter submits with a skill segment in richContent", async () => {
    vi.mocked(commands.listSkills).mockResolvedValue(SKILLS);
    const onSubmit = vi.fn();
    render(
      <RichInput
        onSubmit={onSubmit}
        skillsEnabled={true}
        disabled={false}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    const editable = screen.getByTestId("test-input");
    const user = userEvent.setup();
    await user.click(editable);
    await user.type(editable, "/rev");
    await screen.findByTestId("skill-mention-popup");

    // Filtered down to exactly one match ("reviewer"), so it's already the
    // default-active (index 0) item — no arrow-key press needed to prove
    // Enter confirms *the picker's* selection here (sequential arrow-key
    // advancement itself isn't reliably testable under this project's
    // jsdom setup — research.md's "Resolved (T003 spike)" note; that
    // specific interaction is covered by a WDIO e2e spec instead, T054).
    expect(screen.getAllByTestId("skill-mention-item")).toHaveLength(1);

    await user.keyboard("{Enter}");

    // Confirms RichInput.tsx's own Enter-vs-suggestion-popup race fix: this
    // Enter must have selected the skill, not submitted the message (which
    // would have fired onSubmit with the raw, un-selected "/rev" text).
    expect(onSubmit).not.toHaveBeenCalled();
    expect(screen.queryByTestId("skill-mention-popup")).not.toBeInTheDocument();
    const chip = await screen.findByTestId("skill-mention-chip");
    expect(chip).toHaveTextContent("/reviewer");

    // A second, ordinary Enter (no picker open now) submits — and the
    // skill segment produced by richMessageContentFromDoc (serialize.ts)
    // actually reaches onSubmit's richContent, exactly as it would for the
    // real sendAgentMessage/sendMessage IPC call this wires into.
    await user.keyboard("{Enter}");

    expect(onSubmit).toHaveBeenCalledTimes(1);
    const [, richContent] = onSubmit.mock.calls[0];
    expect(richContent).toBeDefined();
    expect(richContent!.segments).toEqual([
      { type: "skill", id: expect.any(String), name: "reviewer" },
    ]);
  });
});
