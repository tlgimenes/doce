import { describe, it, expect, vi } from "vitest";
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

  it("is reachable via Tab and shows a focus-visible ring", async () => {
    render(<Button>Focusable</Button>);
    const button = screen.getByRole("button", { name: "Focusable" });
    await userEvent.tab();
    expect(button).toHaveFocus();
    expect(button.className).toMatch(/focus-visible:ring/);
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

  it("renders asChild onto the provided element without an extra wrapper", () => {
    render(
      <Button asChild>
        <a href="/somewhere">Link button</a>
      </Button>,
    );
    const link = screen.getByRole("link", { name: "Link button" });
    expect(link.tagName).toBe("A");
    expect(link.className).toContain("cursor-pointer");
  });

  it("marks asChild elements aria-disabled and suppresses their onClick when disabled", async () => {
    const onClick = vi.fn();
    render(
      <Button asChild disabled onClick={onClick}>
        <a href="/somewhere">Disabled link</a>
      </Button>,
    );
    const link = screen.getByRole("link", { name: "Disabled link" });
    expect(link).toHaveAttribute("aria-disabled", "true");

    await userEvent.click(link);
    expect(onClick).not.toHaveBeenCalled();
  });
});
