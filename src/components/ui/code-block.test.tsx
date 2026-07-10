import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { CodeBlock, CodeBlockLine, CodeInline } from "./code-block";

describe("CodeBlock", () => {
  it("renders a mono pre with slot and default tone", () => {
    render(<CodeBlock data-testid="cb">hello</CodeBlock>);
    const el = screen.getByTestId("cb");
    expect(el.tagName).toBe("PRE");
    expect(el).toHaveAttribute("data-slot", "code-block");
    expect(el).toHaveAttribute("data-tone", "default");
    expect(el).toHaveTextContent("hello");
  });

  it("renders the destructive tone", () => {
    render(
      <CodeBlock data-testid="cb" tone="destructive">
        boom
      </CodeBlock>,
    );
    expect(screen.getByTestId("cb")).toHaveAttribute("data-tone", "destructive");
  });

  it("renders diff line variants", () => {
    render(
      <CodeBlock>
        <CodeBlockLine data-testid="l1">ctx</CodeBlockLine>
        <CodeBlockLine data-testid="l2" variant="added">
          plus
        </CodeBlockLine>
        <CodeBlockLine data-testid="l3" variant="removed">
          minus
        </CodeBlockLine>
      </CodeBlock>,
    );
    expect(screen.getByTestId("l1")).toHaveAttribute("data-variant", "default");
    expect(screen.getByTestId("l2")).toHaveAttribute("data-variant", "added");
    expect(screen.getByTestId("l3")).toHaveAttribute("data-variant", "removed");
    expect(screen.getByTestId("l2")).toHaveAttribute("data-slot", "code-block-line");
  });

  it("renders inline code", () => {
    render(<CodeInline data-testid="ci">$ ls</CodeInline>);
    const el = screen.getByTestId("ci");
    expect(el.tagName).toBe("CODE");
    expect(el).toHaveAttribute("data-slot", "code-inline");
  });
});
