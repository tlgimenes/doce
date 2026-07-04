import { createRef, forwardRef, useEffect, useImperativeHandle, useMemo, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { mergeAttributes, Node } from "@tiptap/core";
import { NodeViewWrapper, ReactNodeViewRenderer, type ReactNodeViewProps } from "@tiptap/react";
import Suggestion, {
  type SuggestionKeyDownProps,
  type SuggestionOptions,
  type SuggestionProps,
} from "@tiptap/suggestion";
import { PluginKey, type EditorState } from "@tiptap/pm/state";
import { offset, flip, shift, autoUpdate, useFloating, FloatingPortal } from "@floating-ui/react";
import { commands, type SkillSummary } from "@/lib/ipc";

/**
 * 009-rich-chat-input, User Story 3 (T031): typing "/" opens a picker of
 * the user's locally-installed skills (spec.md's US3 acceptance scenarios;
 * FR-010/FR-012/FR-015); selecting one inserts a marker matching
 * `RichTextSegmentSkill`'s shape (`{ id, name }`, `src/lib/ipc.ts`) at the
 * trigger's range. Same atom-node modeling as `pasted-text-node.tsx`
 * (`Node.create({ group: "inline", inline: true, atom: true })` +
 * `ReactNodeViewRenderer`), plus `@tiptap/suggestion`'s `Suggestion`
 * plugin factory for the "/" trigger/positioning plumbing (research.md's
 * "Skill-mention popup" decision).
 *
 * The popup's actual visual rendering is a plain React tree
 * (`SkillMentionPopup`, below) mounted imperatively through `Suggestion`'s
 * own `render()` lifecycle (`onStart`/`onUpdate`/`onExit` create/update/
 * tear down a `react-dom/client` root appended to `document.body`) rather
 * than a `useReducer`-backed bridge into `RichInput`'s own React tree —
 * research.md explicitly allows either; `render()` was chosen here because
 * it keeps this extension fully self-contained (mounting it into an
 * editor's extensions array is sufficient on its own to get the whole
 * picker working, with nothing for a host component to additionally
 * render), which is what this file's own test harness relies on.
 *
 * Keyboard navigation (T032): ArrowUp/ArrowDown/Enter are handled via the
 * `render()` lifecycle's own `onKeyDown` hook (below), which forwards the
 * raw `KeyboardEvent` to the mounted `SkillMentionPopup` instance through an
 * imperative handle (`moveActive`/`selectActive`) rather than a
 * `@floating-ui/react` `useListNavigation` hook — research.md's "Resolved
 * (T003 spike)" note found `useListNavigation`'s sequential arrow-key
 * advancement unreliable specifically under this project's jsdom test
 * setup, which is a *testability* finding about jsdom, not a reason to
 * avoid arrow-key nav in the real implementation; real users still need it
 * to work, so `skill-mention.test.tsx` intentionally does not assert on
 * sequential arrow-key advancement (jsdom-unreliable per the spike) but the
 * handlers below are still fully wired — verified instead by a WDIO e2e
 * spec (T054, tier 3) driving the real Tauri webview via actual keystrokes.
 *
 * Escape needs no handling here: verified directly against the installed
 * `@tiptap/suggestion` (3.20.2) source — when `render()`'s `onKeyDown`
 * doesn't itself return `true` for an "Escape" keydown, the `Suggestion`
 * plugin already calls `onExit` and dispatches a metadata-only transaction
 * that deactivates the plugin *without touching the document* — exactly
 * "closes without inserting anything, leaving whatever was typed as plain
 * text" (spec.md's US3 scenario), for free.
 *
 * IMPORTANT interaction with `RichInput.tsx`'s own Enter-to-submit handling:
 * confirmed directly against the installed `prosemirror-view` (via
 * `EditorView.someProp`) that `editorProps.handleKeyDown` (passed straight
 * into `new EditorView(dom, {...editorProps})`, i.e. `view._props`) is
 * checked *before* any ProseMirror plugin's own `handleKeyDown` — including
 * this extension's `Suggestion` plugin. Left alone, `RichInput`'s own
 * "Enter (no Shift) submits" handler would win the race and submit the
 * message instead of letting Enter select the highlighted skill.
 * `isSkillMentionSuggestionActive` (exported below) is what `RichInput.tsx`
 * calls to defer to this extension's own key handling while the picker is
 * open.
 */

export interface SkillMentionAttrs {
  id: string;
  name: string;
}

export const skillMentionPluginKey = new PluginKey<{ active: boolean }>("skillMention");

/**
 * Whether the "/" suggestion popup is currently open for the given editor
 * state — `RichInput.tsx`'s `handleKeyDown` calls this before treating
 * Enter as "submit the message" (see this file's top-of-file doc comment
 * for exactly why that ordering matters). Safe to call even when
 * `SkillMention` isn't registered on the editor at all (`skillsEnabled`
 * false): `PluginKey.getState` simply returns `undefined` for a key that
 * isn't part of the state's plugin list, which `?.active` treats as
 * `false`.
 */
export function isSkillMentionSuggestionActive(state: EditorState): boolean {
  return Boolean(skillMentionPluginKey.getState(state)?.active);
}

// --- The inserted marker's chip (live editor + read-only rendering alike,
// mirroring pastedText-node.tsx's NodeView reuse across both). Renders as
// "/{name}" in a small pill, styled with this codebase's own chip tokens
// (`rounded-lg border border-border bg-card`, matching
// `pasted-text-node.tsx`/`004-tool-call-widgets`' widgets) — no click
// interaction: unlike the pastedText chip, a skill marker has nothing to
// expand back into (spec.md's US3 has no such scenario).

function SkillMentionChip({ node }: ReactNodeViewProps) {
  const { name } = node.attrs as SkillMentionAttrs;

  return (
    <NodeViewWrapper as="span" contentEditable={false} data-testid="skill-mention-chip">
      <span className="mx-0.5 inline-flex items-center rounded-lg border border-border bg-card px-1.5 py-0.5 align-baseline text-xs text-foreground">
        {`/${name}`}
      </span>
    </NodeViewWrapper>
  );
}

// --- The popup itself.

interface SkillMentionPopupProps {
  query: string;
  clientRect?: (() => DOMRect | null) | null;
  onSelect: (skill: SkillSummary) => void;
}

/**
 * Imperative surface `createSkillMentionRenderer`'s `onKeyDown` (below)
 * drives: ArrowUp/ArrowDown move the highlighted item (`moveActive`), Enter
 * confirms whichever item is currently highlighted (`selectActive`).
 * Exposed via `useImperativeHandle` rather than owned by the extension's
 * own closure because the filtered item list (what "the next/previous
 * item" even means) is itself derived state that already lives inside this
 * component (the fetched `skills` + the typed `query`) — mirroring it into
 * a second, parallel copy in the closure would risk the two falling out of
 * sync.
 */
export interface SkillMentionPopupHandle {
  moveActive: (delta: number) => void;
  selectActive: () => boolean;
}

function matchesQuery(skill: SkillSummary, needle: string): boolean {
  if (!needle) return true;
  return (
    skill.name.toLowerCase().includes(needle) || skill.description.toLowerCase().includes(needle)
  );
}

function skillMentionItemId(index: number): string {
  return `skill-mention-item-${index}`;
}

/**
 * The picker's actual UI: fetches `list_skills()` once on mount (a plain
 * `useEffect` fetch, no MCP/React-Query machinery — research.md's Data
 * source decision), filters client-side against the typed query (`name`/
 * `description`, case-insensitive substring match), and positions itself
 * via `@floating-ui/react` anchored to the `Suggestion` plugin's decoration
 * node through a virtual element (`clientRect`, updated on every
 * keystroke).
 */
const SkillMentionPopup = forwardRef<SkillMentionPopupHandle, SkillMentionPopupProps>(
  function SkillMentionPopup({ query, clientRect, onSelect }, ref) {
    // `null` = still loading; `[]` = loaded and genuinely empty (FR-015's
    // "no skills installed" case) — kept distinct from a filtered-to-empty
    // list below, which gets its own, differently-worded message.
    const [skills, setSkills] = useState<SkillSummary[] | null>(null);
    const [activeIndex, setActiveIndex] = useState(0);

    useEffect(() => {
      let cancelled = false;
      commands
        .listSkills()
        .then((result) => {
          if (!cancelled) setSkills(result);
        })
        .catch(() => {
          if (!cancelled) setSkills([]);
        });
      return () => {
        cancelled = true;
      };
    }, []);

    const { refs, floatingStyles } = useFloating({
      placement: "bottom-start",
      middleware: [offset(10), flip(), shift()],
      whileElementsMounted: autoUpdate,
    });

    // `clientRect` is a function (not a value) so the anchor position stays
    // live across keystrokes without needing floating-ui to know anything
    // about ProseMirror decorations directly — a virtual element, per
    // floating-ui's own convention for anchoring to something that isn't a
    // real, stable DOM node.
    useEffect(() => {
      refs.setPositionReference({
        getBoundingClientRect: () => clientRect?.() ?? new DOMRect(),
      });
    }, [clientRect, refs]);

    const needle = query.trim().toLowerCase();
    const filtered = useMemo(
      () => (skills ?? []).filter((skill) => matchesQuery(skill, needle)),
      [skills, needle],
    );

    // The filtered list can change shape on every keystroke (new query) or
    // once (skills finish loading) — re-anchoring the highlight to the
    // first item whenever that happens is simplest and avoids a stale index
    // silently pointing at the wrong item (or past the end) after a filter
    // shrinks the list.
    useEffect(() => {
      setActiveIndex(0);
    }, [needle, skills]);

    useImperativeHandle(
      ref,
      () => ({
        moveActive: (delta) => {
          setActiveIndex((current) => {
            if (filtered.length === 0) return 0;
            return (current + delta + filtered.length) % filtered.length;
          });
        },
        selectActive: () => {
          const skill = filtered[activeIndex];
          if (!skill) return false;
          onSelect(skill);
          return true;
        },
      }),
      [filtered, activeIndex, onSelect],
    );

    return (
      <FloatingPortal>
        <div
          ref={refs.setFloating}
          style={floatingStyles}
          data-testid="skill-mention-popup"
          role="listbox"
          aria-activedescendant={filtered.length > 0 ? skillMentionItemId(activeIndex) : undefined}
          className="z-50 max-h-64 w-72 overflow-y-auto rounded-2xl border border-border bg-card p-2 shadow-lg"
        >
          {skills === null ? null : skills.length === 0 ? (
            <p
              className="px-2 py-1 text-sm text-muted-foreground"
              data-testid="skill-mention-empty"
            >
              No skills installed.
            </p>
          ) : filtered.length === 0 ? (
            <p
              className="px-2 py-1 text-sm text-muted-foreground"
              data-testid="skill-mention-empty"
            >
              No matching skills.
            </p>
          ) : (
            <ul className="space-y-0.5">
              {filtered.map((skill, index) => {
                const active = index === activeIndex;
                return (
                  <li key={skill.name}>
                    <button
                      type="button"
                      id={skillMentionItemId(index)}
                      role="option"
                      aria-selected={active}
                      className={`flex w-full flex-col items-start rounded px-2 py-1 text-left text-sm hover:bg-muted ${active ? "bg-muted" : ""}`}
                      data-testid="skill-mention-item"
                      data-active={active}
                      // Prevents the editor from blurring before `onClick`
                      // fires — the click's own `insertContentAt` targets
                      // the trigger's captured `range` directly (not
                      // "wherever the selection currently is"), so this
                      // isn't load-bearing for correctness, only for
                      // avoiding a needless focus-loss/refocus flicker.
                      onMouseDown={(e) => e.preventDefault()}
                      onMouseEnter={() => setActiveIndex(index)}
                      onClick={() => onSelect(skill)}
                    >
                      <span className="font-medium">{skill.name}</span>
                      <span className="text-xs text-muted-foreground">{skill.description}</span>
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      </FloatingPortal>
    );
  },
);

// --- Bridges `Suggestion`'s imperative render() lifecycle to the React
// popup above: `onStart` creates a `react-dom/client` root in a detached
// `<div>` appended to `document.body`; `onUpdate` re-renders it with the
// latest query/position/command; `onExit` unmounts and removes it;
// `onKeyDown` forwards ArrowUp/ArrowDown/Enter to the mounted popup via its
// imperative handle. One renderer (and its own root/container/ref closure)
// per extension instance — `addOptions()` (below) calls this factory fresh
// for every editor that registers the extension, so two concurrent editors
// (e.g. a live `RichInput` and a read-only `UserMessageContent`) never
// share state.

function createSkillMentionRenderer(): NonNullable<
  SuggestionOptions<unknown, SkillMentionAttrs>["render"]
> {
  return () => {
    let root: Root | null = null;
    let container: HTMLDivElement | null = null;
    const popupRef = createRef<SkillMentionPopupHandle>();

    const renderPopup = (props: SuggestionProps<unknown, SkillMentionAttrs>) => {
      root?.render(
        <SkillMentionPopup
          ref={popupRef}
          query={props.query}
          clientRect={props.clientRect}
          onSelect={(skill) => props.command({ id: crypto.randomUUID(), name: skill.name })}
        />,
      );
    };

    return {
      onStart: (props) => {
        container = document.createElement("div");
        document.body.appendChild(container);
        root = createRoot(container);
        renderPopup(props);
      },
      onUpdate: (props) => {
        renderPopup(props);
      },
      // Called by the `Suggestion` plugin's own `handleKeyDown` for every
      // keydown while the popup is active (see `Suggestion`'s installed
      // source: it calls this for every key except when it's already
      // short-circuited Escape's own exit handling above this call).
      // ArrowUp/ArrowDown/Enter are `preventDefault()`-ed and swallowed
      // (return `true`) so they never fall through to ProseMirror's default
      // key bindings (arrow-key caret movement, Enter's paragraph split) —
      // while the picker is open, those keys mean "navigate/confirm the
      // picker," not "edit the document." Any other key (typed characters,
      // Backspace, etc.) is left alone (`return false`) so the query text
      // keeps updating normally.
      onKeyDown: ({ event }: SuggestionKeyDownProps) => {
        if (event.key === "ArrowDown") {
          event.preventDefault();
          popupRef.current?.moveActive(1);
          return true;
        }
        if (event.key === "ArrowUp") {
          event.preventDefault();
          popupRef.current?.moveActive(-1);
          return true;
        }
        if (event.key === "Enter") {
          event.preventDefault();
          // Swallow Enter unconditionally while the picker is open, even if
          // there's currently nothing to select (e.g. "No matching
          // skills") — falling through to ProseMirror's default Enter
          // handling here would insert a paragraph break underneath the
          // still-open popup, which is a worse outcome than a no-op.
          popupRef.current?.selectActive();
          return true;
        }
        return false;
      },
      onExit: () => {
        root?.unmount();
        container?.remove();
        root = null;
        container = null;
      },
    };
  };
}

// --- The node + its "/" trigger plugin.

export interface SkillMentionOptions {
  suggestion: Omit<SuggestionOptions<unknown, SkillMentionAttrs>, "editor">;
}

const SkillMention = Node.create<SkillMentionOptions>({
  name: "skillMention",
  group: "inline",
  inline: true,
  atom: true,

  addOptions() {
    return {
      suggestion: {
        char: "/",
        pluginKey: skillMentionPluginKey,
        // Replaces the matched "/query" range with the inserted node —
        // FR-012's "insert a visible marker ... at the point of
        // selection." `id` is generated by the caller at selection time
        // (the popup's `onSelect`, above), matching `pastedText`'s own
        // convention (`RichInput.tsx`'s `handlePaste` generates its chip's
        // `id` the same way) rather than being generated inside the node.
        command: ({ editor, range, props }) => {
          editor
            .chain()
            .focus()
            .insertContentAt(range, {
              type: "skillMention",
              attrs: { id: props.id, name: props.name },
            })
            .run();
        },
        render: createSkillMentionRenderer(),
      },
    };
  },

  addAttributes() {
    return {
      id: {
        default: null,
        parseHTML: (element) => element.getAttribute("data-id"),
        renderHTML: (attributes) => ({ "data-id": attributes.id }),
      },
      name: {
        default: "",
        parseHTML: (element) => element.getAttribute("data-name") ?? "",
        renderHTML: (attributes) => ({ "data-name": attributes.name }),
      },
    };
  },

  parseHTML() {
    return [{ tag: `span[data-type="${this.name}"]` }];
  },

  renderHTML({ HTMLAttributes }) {
    return ["span", mergeAttributes({ "data-type": this.name }, HTMLAttributes)];
  },

  addNodeView() {
    return ReactNodeViewRenderer(SkillMentionChip);
  },

  addProseMirrorPlugins() {
    return [
      Suggestion({
        editor: this.editor,
        ...this.options.suggestion,
      }),
    ];
  },
});

export default SkillMention;
