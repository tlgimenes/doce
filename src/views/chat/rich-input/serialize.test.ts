import { describe, it, expect } from "vitest";
import {
  shouldCollapsePastedText,
  richMessageContentFromDoc,
  richMessageContentToDoc,
} from "./serialize";
import type { RichTextSegmentSkill, RichTextSegmentAttachment } from "@/lib/ipc";

/**
 * 009-rich-chat-input, User Story 2 (T022): pure, DOM-free tests for the
 * paste-collapse threshold decision that RichInput.tsx's `handlePaste`
 * plugin (T023) will call. FR-003 / research.md's "Paste-collapse via
 * editorProps.handlePaste" decision: a paste collapses into a `pastedText`
 * chip once it exceeds roughly 10 lines OR roughly 500 characters,
 * whichever is reached first — mirroring research.md's own idiom
 * (`.split("\n").length > 10`).
 *
 * Boundary convention: spec.md's FR-003 says "exceeds roughly 10 lines or
 * 500 characters" and the User Story 2 narrative says "longer than
 * roughly 10 lines or 500 characters" — both are "more than" language, not
 * "at least"/"10 or more". So the threshold triggers only when a value is
 * STRICTLY GREATER than the limit: text at exactly 10 lines and exactly
 * 500 characters does NOT collapse.
 */
describe("shouldCollapsePastedText (009-rich-chat-input, US2)", () => {
  it("does not collapse text under both thresholds", () => {
    // 5 lines, well under 500 characters.
    const text = ["line one", "line two", "line three", "line four", "line five"].join("\n");

    const result = shouldCollapsePastedText(text);

    expect(result.shouldCollapse).toBe(false);
    expect(result.lineCount).toBe(5);
  });

  it("collapses text that crosses the line threshold while staying under the char threshold", () => {
    // 15 one-character lines: lineCount (15) > 10, but total length (15
    // chars + 14 newlines = 29) is far under 500.
    const text = Array.from({ length: 15 }, () => "x").join("\n");

    const result = shouldCollapsePastedText(text);

    expect(text.length).toBeLessThan(500);
    expect(result.lineCount).toBe(15);
    expect(result.shouldCollapse).toBe(true);
  });

  it("collapses text that crosses the char threshold while staying under the line threshold", () => {
    // A single 600-character line: no newlines at all (lineCount stays 1),
    // but length (600) exceeds 500.
    const text = "a".repeat(600);

    const result = shouldCollapsePastedText(text);

    expect(result.lineCount).toBe(1);
    expect(result.shouldCollapse).toBe(true);
  });

  it("does not collapse text at exactly the boundary: exactly 10 lines and exactly 500 characters", () => {
    // 10 lines (9 newline separators): 9 lines of 1 char ("a") + 1 line of
    // 482 chars => 9 + 482 = 491 content characters + 9 newlines = 500
    // characters total, in exactly 10 lines.
    const lines = [...Array(9).fill("a"), "b".repeat(482)];
    const text = lines.join("\n");

    expect(text.length).toBe(500);

    const result = shouldCollapsePastedText(text);

    expect(result.lineCount).toBe(10);
    expect(result.shouldCollapse).toBe(false);
  });

  it("collapses text one line past the line threshold (11 lines), even with a tiny char count", () => {
    const text = Array.from({ length: 11 }, () => "x").join("\n");

    expect(text.length).toBeLessThan(500);

    const result = shouldCollapsePastedText(text);

    expect(result.lineCount).toBe(11);
    expect(result.shouldCollapse).toBe(true);
  });

  it("collapses text one character past the char threshold (501 chars), on a single line", () => {
    const text = "a".repeat(501);

    const result = shouldCollapsePastedText(text);

    expect(result.lineCount).toBe(1);
    expect(result.shouldCollapse).toBe(true);
  });

  it("does not collapse empty text", () => {
    const result = shouldCollapsePastedText("");

    expect(result.lineCount).toBe(1);
    expect(result.shouldCollapse).toBe(false);
  });
});

/**
 * 009-rich-chat-input, User Story 3 (T024 completion): direct, DOM-free
 * unit tests for `richMessageContentFromDoc`/`richMessageContentToDoc`'s
 * `skillMention` node <-> `"skill"` segment handling (skill-mention.tsx,
 * T031/T033) — the doc<->JSON conversion these two pure functions do is
 * exactly what `RichInput.tsx`'s submit path and `UserMessageContent.tsx`'s
 * read-only rendering path rely on for a message containing a skill
 * mention, so it's covered here directly rather than only indirectly via a
 * full editor mount.
 */
describe("richMessageContentFromDoc (009-rich-chat-input, US3 — skillMention node)", () => {
  it("converts a skillMention node into a matching skill segment", () => {
    const doc = {
      type: "doc",
      content: [
        {
          type: "paragraph",
          content: [{ type: "skillMention", attrs: { id: "skill-1", name: "reviewer" } }],
        },
      ],
    };

    const result = richMessageContentFromDoc(doc);

    expect(result.segments).toEqual([{ type: "skill", id: "skill-1", name: "reviewer" }]);
  });

  it("orders a skillMention node correctly relative to surrounding plain text", () => {
    const doc = {
      type: "doc",
      content: [
        {
          type: "paragraph",
          content: [
            { type: "text", text: "please use " },
            { type: "skillMention", attrs: { id: "skill-2", name: "planner" } },
            { type: "text", text: " for this" },
          ],
        },
      ],
    };

    const result = richMessageContentFromDoc(doc);

    expect(result.segments).toEqual([
      { type: "text", text: "please use " },
      { type: "skill", id: "skill-2", name: "planner" },
      { type: "text", text: " for this" },
    ]);
  });
});

describe("richMessageContentToDoc (009-rich-chat-input, US3 — skill segment)", () => {
  it("converts a skill segment into a matching skillMention node", () => {
    const skillSegment: RichTextSegmentSkill = { type: "skill", id: "skill-3", name: "reviewer" };

    const doc = richMessageContentToDoc({ segments: [skillSegment] });

    expect(doc).toEqual({
      type: "doc",
      content: [
        {
          type: "paragraph",
          content: [{ type: "skillMention", attrs: { id: "skill-3", name: "reviewer" } }],
        },
      ],
    });
  });

  it("round-trips a skill segment placed between plain-text segments", () => {
    const content = {
      segments: [
        { type: "text" as const, text: "before " },
        { type: "skill" as const, id: "skill-4", name: "planner" },
        { type: "text" as const, text: " after" },
      ],
    };

    const doc = richMessageContentToDoc(content);

    expect(doc).toEqual({
      type: "doc",
      content: [
        {
          type: "paragraph",
          content: [
            { type: "text", text: "before " },
            { type: "skillMention", attrs: { id: "skill-4", name: "planner" } },
            { type: "text", text: " after" },
          ],
        },
      ],
    });
  });
});

/**
 * 009-rich-chat-input, User Story 4 (serialize.ts's attachment segment
 * support): direct, DOM-free unit tests for `richMessageContentFromDoc`/
 * `richMessageContentToDoc`'s `attachment` node <-> `"attachment"` segment
 * handling (attachment-node.tsx), mirroring the `skillMention`/`skill`
 * coverage above exactly. `RichInput.tsx`'s submit path and (once wired)
 * `UserMessageContent.tsx`'s read-only rendering both rely on this
 * conversion being lossless for every one of the segment's five fields
 * (`id`/`name`/`mimeType`/`data`/`isImage`) — including the base64 `data`
 * payload itself, which is the one field FR-009/`expand_segments`
 * (rich_content.rs) is explicit must never leak into anything model-facing;
 * this file's conversion is purely doc<->JSON, entirely separate from that
 * concern, so round-tripping it here is exactly as safe as round-tripping
 * any other attribute.
 */
describe("richMessageContentFromDoc (009-rich-chat-input, US4 — attachment node)", () => {
  it("converts an attachment node into a matching attachment segment", () => {
    const doc = {
      type: "doc",
      content: [
        {
          type: "paragraph",
          content: [
            {
              type: "attachment",
              attrs: {
                id: "att-1",
                name: "photo.png",
                mimeType: "image/png",
                data: "ZmFrZS1pbWFnZS1ieXRlcw==",
                isImage: true,
              },
            },
          ],
        },
      ],
    };

    const result = richMessageContentFromDoc(doc);

    expect(result.segments).toEqual([
      {
        type: "attachment",
        id: "att-1",
        name: "photo.png",
        mimeType: "image/png",
        data: "ZmFrZS1pbWFnZS1ieXRlcw==",
        isImage: true,
      },
    ]);
  });

  it("orders an attachment node correctly relative to surrounding plain text", () => {
    const doc = {
      type: "doc",
      content: [
        {
          type: "paragraph",
          content: [
            { type: "text", text: "see " },
            {
              type: "attachment",
              attrs: {
                id: "att-2",
                name: "report.pdf",
                mimeType: "application/pdf",
                data: "ZmFrZS1wZGYtYnl0ZXM=",
                isImage: false,
              },
            },
            { type: "text", text: " for details" },
          ],
        },
      ],
    };

    const result = richMessageContentFromDoc(doc);

    expect(result.segments).toEqual([
      { type: "text", text: "see " },
      {
        type: "attachment",
        id: "att-2",
        name: "report.pdf",
        mimeType: "application/pdf",
        data: "ZmFrZS1wZGYtYnl0ZXM=",
        isImage: false,
      },
      { type: "text", text: " for details" },
    ]);
  });
});

describe("richMessageContentToDoc (009-rich-chat-input, US4 — attachment segment)", () => {
  it("converts an attachment segment into a matching attachment node", () => {
    const attachmentSegment: RichTextSegmentAttachment = {
      type: "attachment",
      id: "att-3",
      name: "photo.png",
      mimeType: "image/png",
      data: "ZmFrZS1pbWFnZS1ieXRlcw==",
      isImage: true,
    };

    const doc = richMessageContentToDoc({ segments: [attachmentSegment] });

    expect(doc).toEqual({
      type: "doc",
      content: [
        {
          type: "paragraph",
          content: [
            {
              type: "attachment",
              attrs: {
                id: "att-3",
                name: "photo.png",
                mimeType: "image/png",
                data: "ZmFrZS1pbWFnZS1ieXRlcw==",
                isImage: true,
              },
            },
          ],
        },
      ],
    });
  });

  it("round-trips an attachment segment placed between plain-text segments, all five fields intact", () => {
    const content = {
      segments: [
        { type: "text" as const, text: "before " },
        {
          type: "attachment" as const,
          id: "att-4",
          name: "notes.txt",
          mimeType: "text/plain",
          data: "aGVsbG8=",
          isImage: false,
        },
        { type: "text" as const, text: " after" },
      ],
    };

    const doc = richMessageContentToDoc(content);
    const roundTripped = richMessageContentFromDoc(doc);

    expect(roundTripped.segments).toEqual(content.segments);
  });
});
