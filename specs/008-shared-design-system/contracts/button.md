# Contract: `Button` component

**Module**: `@/components/ui/button` — exports `Button` and `buttonVariants` (the latter so non-Button elements that must look like a button, e.g. an `asChild` anchor, can reuse the class map).

```ts
type ButtonVariant = "primary" | "secondary" | "destructive" | "ghost";
type ButtonSize = "sm" | "md";

interface ButtonProps extends React.ComponentPropsWithoutRef<"button"> {
  variant?: ButtonVariant; // default "primary"
  size?: ButtonSize;       // default "md"
  asChild?: boolean;       // default false
}

function Button(props: ButtonProps): JSX.Element;
```

## Behavioral contract

- Renders a native `<button>` by default; when `asChild` is true,
  clones its single child and applies the same class list + behavior
  to it instead (for the "must be an `<a>`" case) via Radix's `Slot`.
- All native `button` props (`onClick`, `type`, `data-testid`,
  `aria-*`, `id`, ...) pass through unmodified — the component never
  strips or renames a prop it doesn't recognize.
- `disabled` (or `aria-disabled` when `asChild` renders a non-button
  element that has no native `disabled`) MUST suppress pointer-cursor
  and hover/active styling (FR-002/FR-003) and MUST NOT fire `onClick`.
- Keyboard: Enter and Space activate the button when it has focus
  (native `<button>` behavior; preserved when `asChild` is used because
  Radix `Slot` does not intercept native semantics).
- Focus: a visible focus ring (`--color-ring` token) appears on
  keyboard focus (`:focus-visible`), not on mouse click focus.
- Does not manage its own loading/busy state — a caller wanting a
  "submitting" button passes `disabled` and their own visual indicator
  (e.g. a spinner as `children`); out of scope for this pass.

## Compatibility contract (migration)

- Any existing call site passing `className`, `data-testid`, `onClick`,
  or `disabled` to a hand-rolled `<button>` continues to work
  identically after swapping to `<Button>` — this is what makes the
  migration in User Story 3 behavior-preserving (FR-010).
