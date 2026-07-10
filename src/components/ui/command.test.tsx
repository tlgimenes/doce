import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";

import { Command, CommandItem, CommandList } from "./command";

describe("CommandItem", () => {
  it("does not trigger extra command rerenders when keywords are omitted", async () => {
    const renderProbe = vi.fn();

    function RenderProbe() {
      renderProbe();
      return null;
    }

    const { rerender } = render(
      <Command data-tick="0">
        <RenderProbe />
        <CommandList>
          <CommandItem>Open settings</CommandItem>
        </CommandList>
      </Command>,
    );
    expect(await screen.findByRole("button", { name: "Open settings" })).toBeInTheDocument();

    renderProbe.mockClear();
    rerender(
      <Command data-tick="1">
        <RenderProbe />
        <CommandList>
          <CommandItem>Open settings</CommandItem>
        </CommandList>
      </Command>,
    );

    expect(renderProbe).toHaveBeenCalledTimes(1);
  });
});
