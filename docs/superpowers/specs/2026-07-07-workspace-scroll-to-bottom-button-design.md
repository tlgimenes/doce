# Workspace Scroll To Bottom Button Design

## Goal

When the user scrolls upward in the Workspace transcript and autoscroll detaches, show a small floating rounded-full arrow-down button at the bottom-right of the transcript. Clicking it scrolls to the bottom immediately and reactivates pinned autoscroll.

## Scope

In scope:

- Add a floating scroll-to-bottom affordance to `src/views/workspace/Workspace.tsx`.
- Render the button only while Workspace autoscroll is detached.
- Place it inside the transcript area, bottom-right, visually above the composer.
- Use an arrow-down icon from the existing icon library.
- Add focused unit tests in `src/views/workspace/Workspace.test.tsx`.

Out of scope:

- New unread-message counters.
- Animation beyond standard hover/focus/transition styling.
- Changes to composer layout.
- Changes to e2e coverage.

## Behavior

Workspace already tracks pinned autoscroll in `autoscrollPinnedRef`. The button needs visible React state too, because refs do not trigger rendering.

Add `const [isAutoscrollPinned, setIsAutoscrollPinned] = useState(true);`.

Whenever autoscroll pinning changes:

- Update `autoscrollPinnedRef.current`.
- Update `isAutoscrollPinned`.

On conversation switch:

- Reset both the ref and state to pinned.
- Hide the button.
- Keep the existing scheduled scroll-to-bottom behavior.

When the user scrolls up past the existing 48px threshold:

- Set pinned false.
- Show the button.
- New messages should not force-scroll the transcript.

When the user scrolls back near the bottom:

- Set pinned true.
- Hide the button.
- Existing autoscroll behavior resumes.

When the user clicks the button:

- Set pinned true in both ref and state.
- Scroll the transcript to the bottom immediately.
- Keep future transcript updates pinned.

## UI

The button renders only when `!isAutoscrollPinned`.

Placement:

- The transcript container should become a positioning context with `relative`.
- The button should be absolutely positioned inside the transcript area, bottom-right.
- Use spacing that keeps it clear of transcript content and visually above the composer, for example `bottom-4 right-4`.

Visual style:

- Rounded full icon button.
- Compact size, matching the app's existing small icon-button scale.
- Use `ArrowDown` from `@phosphor-icons/react`.
- Include an accessible label such as `Scroll to bottom`.
- Add `data-testid="scroll-to-bottom"`.

## Testing

Add focused tests in `Workspace.test.tsx`:

- The button appears when the user scrolls up and autoscroll detaches.
- The button hides when the user scrolls back near the bottom.
- Clicking the button scrolls the transcript to bottom and reactivates autoscroll.
- After clicking the button, a later message follows the bottom again.

The tests should reuse the existing jsdom scroll metric helpers and animation-frame flush helpers.

## Risks

The button state must stay in sync with `autoscrollPinnedRef`. A helper that sets both values should be preferred over manually assigning them in multiple places.

The click handler must scroll immediately rather than waiting for a future transcript update, otherwise the affordance feels broken.
