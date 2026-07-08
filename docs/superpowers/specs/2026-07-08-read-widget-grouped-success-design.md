# ReadWidget grouped success UI

**Status**: Approved
**Context**: Follow-up to the tool-call widget work and the visual comparison shown in the brainstorming companion. The current `ReadWidget` has one successful render branch but the gallery/tests still frame success, truncated, and offloaded reads as separate visual states. The selected direction is option 3, "Pure File Reference", with one refinement: each successful read should display token count when available.

## Motivation

The Read tool should read as a compact file-reference event, not as a multi-state status component. A normal read, a capped/truncated read, and an offloaded read are all successful file reads; making them look like separate states adds noise to the conversation. The only visually distinct state should be failure, where the user needs a clear error.

At the same time, token cost is useful context for every read. The UI should preserve the already-approved per-widget cost visibility by showing token count on each new successful Read result.

## Scope

- Group successful, truncated, and offloaded Read results into one minimal successful-read UI.
- Keep failure as the only distinct Read visual state.
- Show byte count and token count for successful reads when `tokenCount` is present.
- Ensure newly recorded successful Read results have `tokenCount`; if the existing token-count annotation path does not already cover Read, fixing that plumbing is in scope without changing the frontend data shape.
- Keep `ViewFullOutput` available when `offloadedTo` is present.
- Update the design-system gallery so it no longer teaches truncation as a separate Read state.
- Update focused unit tests for the new visible behavior.

Out of scope:

- Backend data-model changes.
- New IPC commands.
- Changing how offloaded files are read.
- Adding content previews to `ReadWidget`.
- Changing `BashWidget` or other tool widgets.

## UI Behavior

### Successful reads

Every successful read renders as the same quiet card:

```text
Read <path> · <bytes> · <tokens> tok
```

The card is intentionally metadata-only. It does not show file content, a preview, or a separate success label.

Metadata rules:

- Byte count is derived from `detail.outcome.content.length`, as today.
- Token count appears when `detail.tokenCount != null`.
- For older persisted rows without `tokenCount`, the token segment is omitted rather than estimated.
- Metadata order is bytes first, tokens second.

### Truncated reads

`detail.outcome.truncated` remains part of the data contract but no longer creates a visible "Output truncated" row or a separate visual state. A truncated read still renders through the same successful-read card.

### Offloaded reads

`detail.offloadedTo` remains an optional affordance, not a state. When present, the same successful-read card also shows `ViewFullOutput`.

The implementation should keep using the existing shared `ViewFullOutput` component. Its current secondary row treatment is acceptable, but it must not add warning/destructive styling, an offloaded badge, or any separate "offloaded" state header. It must still support the existing loading, loaded, and error behavior from `ViewFullOutput`.

### Failed reads

Failures keep their existing separate destructive treatment:

- Red/destructive border/background.
- `Read <path>` line.
- Error text from `detail.outcome.error`.

Failures do not show byte/token metadata because there is no successful content payload to measure.

## Data Flow

No new data is introduced.

`ReadWidget` continues to consume `ReadDetail`:

- `outcome.ok === false`: render failure card.
- `outcome.ok === true`: render grouped successful-read card.
- `outcome.truncated`: retained but not visually labeled.
- `offloadedTo`: optional full-output affordance.
- `tokenCount`: optional metadata for successful reads.

Newly persisted Read results must include `tokenCount` via the existing token-count annotation work. If implementation finds that Read is not currently covered, the implementation should fix that annotation path. The frontend remains tolerant of older rows without `tokenCount`.

## Gallery Updates

`WidgetGallery.tsx` should stop presenting Read as "Success / truncated / offloaded / failure". The Read section should instead show examples that communicate the grouped model, such as:

- Standard read with byte/token metadata.
- Offloaded read with the same metadata plus `View full output`.
- Failure.

A truncated-only example is no longer necessary unless it is explicitly labeled as using the same successful-read UI.

## Testing

Update `ReadWidget.test.tsx` around visible behavior:

- A successful read renders the compact file-reference card and path.
- A successful read with `tokenCount` renders the token segment.
- A successful read without `tokenCount` omits the token segment.
- A truncated successful read does not render `read-truncated` or "Output truncated".
- An offloaded successful read still renders the `ViewFullOutput` affordance.
- A failed read still renders the destructive failure state and error message.

If `WidgetGallery` has assertions tied to the old Read labels, update them to match the grouped examples.

## Acceptance Criteria

- Success, truncated, and offloaded Read results share one successful-read visual treatment.
- Truncated reads no longer display "Output truncated".
- Successful Read results display token count whenever `tokenCount` is present.
- Offloaded Read results still let the user view the full output.
- Failure remains visually distinct and unchanged in meaning.
- Existing persisted rows without `tokenCount` render cleanly.
