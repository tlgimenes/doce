import "@testing-library/jest-dom/vitest";
import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";

// Not automatic under vitest by default — without this, each test's render
// output accumulates in the DOM instead of being torn down, which broke
// exactly this way: a second test's `findByTestId` failed because the
// previous test's still-mounted component had a matching element too.
//
// The extra tick after cleanup() exists for @tiptap/react specifically:
// its useEditor() hook always defers the real editor.destroy() by
// setTimeout(..., 1) (to survive React StrictMode's double-invoke without
// tearing down a still-mounted editor — see its scheduleDestroy()).
// cleanup() only unmounts synchronously, so that deferred destroy still
// fires later; if it fires after vitest has already moved on to tearing
// down this test file's jsdom environment (observed on a slower CI
// runner, not reproducible on a fast local machine), it throws
// "ReferenceError: window is not defined" as an *unhandled* exception —
// individual tests still pass, but it flips the whole run's exit code to
// 1, the same class of silent-poisoning issue documented below for
// unmocked ProseMirror exceptions.
afterEach(async () => {
  cleanup();
  await new Promise((resolve) => setTimeout(resolve, 10));
});

// jsdom's <dialog> implementation is a bare stub with no showModal()/close()
// (https://github.com/jsdom/jsdom/issues/3294) — confirmed directly:
// `dialog.showModal()` throws "not a function" as of jsdom 29. Dialog.tsx
// (005-keyboard-shortcuts) is this app's first use of the element, so
// without this polyfill every test that renders it crashes the moment
// `open` becomes true. The `open` attribute/property reflection itself
// already works in jsdom (auto-generated from the IDL) — only the two
// interactive methods are missing, so that's all this adds.
if (!HTMLDialogElement.prototype.showModal) {
  HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
    this.setAttribute("open", "");
  };
  HTMLDialogElement.prototype.close = function (this: HTMLDialogElement) {
    this.removeAttribute("open");
  };
}

// jsdom has no layout engine, and (confirmed directly against jsdom 29.1.1)
// doesn't even implement Range.prototype.getBoundingClientRect/getClientRects
// or document.elementFromPoint at all (both `undefined`, not just
// zero-returning). ProseMirror's EditorView calls these on essentially every
// transaction (coordsAtPos -> scrollToSelection) and on mousedown
// (posAtCoords) — 009-rich-chat-input's Tiptap-based input is this app's
// first use of a contenteditable-backed editor, and without these, ordinary
// typing throws an uncaught TypeError that doesn't fail an individual
// expect() but does flip `vitest run`'s exit code to 1, silently poisoning
// an otherwise-green suite.
if (!Range.prototype.getBoundingClientRect) {
  Range.prototype.getBoundingClientRect = function (this: Range) {
    return {
      top: 0,
      left: 0,
      right: 0,
      bottom: 0,
      width: 0,
      height: 0,
      x: 0,
      y: 0,
      toJSON() {},
    } as DOMRect;
  };
}
if (!Range.prototype.getClientRects) {
  Range.prototype.getClientRects = function (this: Range) {
    return {
      length: 0,
      item: () => null,
      [Symbol.iterator]: [][Symbol.iterator],
    } as unknown as DOMRectList;
  };
}
if (!document.elementFromPoint) {
  document.elementFromPoint = () => null;
}

// jsdom 29 has no native ResizeObserver at all. Not exercised as a hard
// crash by Tiptap/StarterKit or @floating-ui/react's `autoUpdate` (both
// feature-detect its absence and no-op), but cheap to stub proactively
// since a future custom node view or floating-ui option may assume it
// exists.
if (typeof globalThis.ResizeObserver === "undefined") {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
}

// jsdom has no IntersectionObserver either; the @shadcn/react
// message-scroller needs one to exist. Inert stub — autoscroll behavior is
// not testable in jsdom and is covered by browser-level verification.
if (typeof globalThis.IntersectionObserver === "undefined") {
  globalThis.IntersectionObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
    takeRecords() {
      return [];
    }
  } as unknown as typeof IntersectionObserver;
}

// jsdom has no layout engine, so Element.prototype.scrollIntoView is
// entirely absent (not just a no-op). cmdk's <Command> calls it in a layout
// effect every time the highlighted item changes (mount, filter, arrow keys)
// to keep the highlighted option in view — confirmed directly: without this
// stub, every test that mounts stock cmdk (Task 5's command.tsx) throws
// "scrollIntoView is not a function" from inside cmdk's effect, which
// (like the other unhandled-exception cases documented above) doesn't fail
// an individual expect() but flips the whole `vitest run` exit code to 1.
if (!Element.prototype.scrollIntoView) {
  Element.prototype.scrollIntoView = function () {};
}

// jsdom has no matchMedia implementation. next-themes' ThemeProvider calls
// window.matchMedia("(prefers-color-scheme: dark)") on mount (to resolve
// "system" theme) regardless of whether a test cares about theming, so
// without this stub every suite that renders anything under the app's
// ThemeProvider (main.tsx) throws "matchMedia is not a function".
if (typeof window.matchMedia === "undefined") {
  window.matchMedia = ((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: () => {},
    removeListener: () => {},
    addEventListener: () => {},
    removeEventListener: () => {},
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
}
