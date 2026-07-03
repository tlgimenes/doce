import "@testing-library/jest-dom/vitest";
import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";

// Not automatic under vitest by default — without this, each test's render
// output accumulates in the DOM instead of being torn down, which broke
// exactly this way: a second test's `findByTestId` failed because the
// previous test's still-mounted component had a matching element too.
afterEach(() => {
  cleanup();
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
