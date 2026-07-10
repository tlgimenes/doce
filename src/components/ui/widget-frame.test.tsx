import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { ItemContent, ItemTitle } from "@/components/ui/item";
import { WidgetFrame, WidgetFrameContent, WidgetFrameHeader } from "./widget-frame";

describe("WidgetFrame", () => {
  it("renders a header-only card without a trigger", () => {
    render(
      <WidgetFrame data-testid="frame">
        <WidgetFrameHeader>
          <ItemContent>
            <ItemTitle>plain card</ItemTitle>
          </ItemContent>
        </WidgetFrameHeader>
      </WidgetFrame>,
    );
    expect(screen.getByTestId("frame")).toHaveAttribute("data-slot", "widget-frame");
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(screen.getByText("plain card")).toBeInTheDocument();
  });

  it("collapsed by default: body hidden until the header is clicked", async () => {
    render(
      <WidgetFrame collapsible data-testid="frame">
        <WidgetFrameHeader>
          <ItemContent>
            <ItemTitle>summary</ItemTitle>
          </ItemContent>
        </WidgetFrameHeader>
        <WidgetFrameContent>body text</WidgetFrameContent>
      </WidgetFrame>,
    );
    const trigger = screen.getByRole("button");
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    // Base UI's Collapsible.Panel unmounts hidden content by default (no
    // `keepMounted`), so the closed body isn't in the DOM at all rather than
    // present-but-hidden.
    expect(screen.queryByText("body text")).not.toBeInTheDocument();

    await userEvent.click(trigger);
    expect(trigger).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("body text")).toBeVisible();
  });

  it("defaultOpen renders the body expanded", () => {
    render(
      <WidgetFrame collapsible defaultOpen>
        <WidgetFrameHeader>
          <ItemContent>
            <ItemTitle>summary</ItemTitle>
          </ItemContent>
        </WidgetFrameHeader>
        <WidgetFrameContent>open body</WidgetFrameContent>
      </WidgetFrame>,
    );
    expect(screen.getByRole("button")).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("open body")).toBeVisible();
  });
});
