/// <reference types="node" />

import { describe, it, expect, vi } from "vitest";
import { readFileSync } from "node:fs";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Button } from "./button";

describe("Button", () => {
  it("renders children and a native button by default", () => {
    render(<Button>Click me</Button>);
    expect(screen.getByRole("button", { name: "Click me" })).toBeInTheDocument();
  });

  it("applies pointer-cursor and hover classes when enabled", () => {
    render(<Button>Enabled</Button>);
    const button = screen.getByRole("button", { name: "Enabled" });
    expect(button.className).toContain("cursor-pointer");
    expect(button.className).toMatch(/hover:/);
  });

  it("does not apply pointer-cursor styling when disabled and blocks onClick", async () => {
    const onClick = vi.fn();
    render(
      <Button disabled onClick={onClick}>
        Disabled
      </Button>,
    );
    const button = screen.getByRole("button", { name: "Disabled" });
    expect(button).toBeDisabled();
    expect(button.className).toContain("disabled:cursor-not-allowed");

    await userEvent.click(button);
    expect(onClick).not.toHaveBeenCalled();
  });

  it("merges a caller-supplied className without dropping base classes", () => {
    render(<Button className="mt-4">Styled</Button>);
    const button = screen.getByRole("button", { name: "Styled" });
    expect(button.className).toContain("mt-4");
    expect(button.className).toContain("rounded-md");
  });

  it.each([
    ["primary", "bg-primary"],
    ["secondary", "border-border"],
    ["destructive", "bg-destructive"],
    ["ghost", "bg-transparent"],
  ] as const)("applies %s variant classes", (variant, expectedClass) => {
    render(<Button variant={variant}>Variant</Button>);
    expect(screen.getByRole("button", { name: "Variant" }).className).toContain(expectedClass);
  });

  it.each([
    ["icon", "size-8"],
    ["icon-sm", "size-6"],
  ] as const)("applies %s icon size classes", (size, expectedClass) => {
    render(
      <Button size={size} aria-label={size}>
        <span aria-hidden="true">×</span>
      </Button>,
    );
    const button = screen.getByRole("button", { name: size });
    expect(button.className).toContain(expectedClass);
    expect(button.className).toContain("p-0");
  });

  it("is reachable via Tab and relies on the app-global focus outline", async () => {
    render(<Button>Focusable</Button>);
    const button = screen.getByRole("button", { name: "Focusable" });
    await userEvent.tab();
    expect(button).toHaveFocus();
    expect(button.className).not.toMatch(/focus-visible:(outline-none|ring)/);
  });

  it("activates via Enter and Space when focused", async () => {
    const onClick = vi.fn();
    render(<Button onClick={onClick}>Keyboard</Button>);
    const button = screen.getByRole("button", { name: "Keyboard" });
    button.focus();

    await userEvent.keyboard("{Enter}");
    expect(onClick).toHaveBeenCalledTimes(1);

    await userEvent.keyboard(" ");
    expect(onClick).toHaveBeenCalledTimes(2);
  });

  it("does not import Radix Slot", () => {
    const source = readFileSync("src/components/ui/button.tsx", "utf8");
    expect(source).not.toContain("@radix-ui/react-slot");
    expect(source).not.toContain("Slot");
  });
});
