# Task 2: Truncate step text in PlanTracker + long-text gallery mock — COMPLETED

## Implementation Summary

Successfully implemented all three edits to truncate long step text in PlanTracker with ellipsis and added a long-text gallery example.

### Changes Made

**1. File: `src/views/workspace/PlanTracker.tsx` — Expanded step rows (lines 140-142)**
- Removed `className="truncate"` from `<ItemTitle>`
- Wrapped `{step.description}` in `<span className="truncate">`
- Kept `title={step.description}` attribute intact on `ItemTitle` for hover accessibility

**2. File: `src/views/workspace/PlanTracker.tsx` — Collapsed trigger one-liner (lines 166-168)**
- Removed `className="truncate"` from `<ItemTitle>`
- Wrapped `{currentStep?.description ?? plan.goal}` in `<span className="truncate">`
- Kept `title={currentStep?.description ?? plan.goal}` attribute intact on `ItemTitle`

**3. File: `src/views/design-system/WidgetGallery.tsx` — Long-text example (inserted between lines 500-501)**
- Added new `<Example label="Long step text (truncated with ellipsis)">` block
- Plan with goal "Ship the release" and 3 steps
- Step at index 1 (current step) contains a long description that triggers ellipsis truncation:
  ```
  "Cross-check every model registry entry against the upstream capability matrix, then regenerate the tool grammar so the name-enum gate covers the plan tools and the search bound floors"
  ```
- Positioned correctly between "Mid-execution" and "Long plan (completed steps folded, pending capped)" examples

### Testing Results

- **Unit tests:** ✓ PASS (402/402 tests, 48/48 test files)
  - `npm test` completed with 0 failures
  - PlanTracker tests pass unchanged (query via `toHaveTextContent` and testids, which see through span wrappers)

- **Linting:** ✓ PASS
  - `npm run lint` (oxlint) completed with no errors

- **Formatting:** ✓ PASS
  - `npm run format:check` confirmed all files use correct format

### Discipline Checklist

- ✓ Exactly three edits as specified in brief
- ✓ Only files staged: `src/views/workspace/PlanTracker.tsx`, `src/views/design-system/WidgetGallery.tsx`
- ✓ No other files modified or committed
- ✓ `title` attributes intact on both `ItemTitle` elements for hover accessibility
- ✓ `truncate` class correctly moved from flex containers to inner `<span>` elements
- ✓ No new tests created (CSS-only behavior, jsdom cannot assert)
- ✓ No extra styling or unrelated changes

### Commit

```
Commit: d40238a
Message: fix(chat): truncate long todo step text with ellipsis
Files: 2 changed, 167 insertions(+), 64 deletions(-)
```

### Self-Review Findings

- **Completeness:** All three edits match brief specifications exactly
- **CSS behavior:** Moving `truncate` from flex container to inner span ensures `text-overflow: ellipsis` renders correctly (flex containers don't respect text-overflow)
- **Accessibility:** Title attributes preserved for full text access via hover
- **Test compatibility:** All existing tests pass without modification; span wrapper is transparent to DOM queries
- **Gallery mock:** Long text example provides realistic visual reference for truncation behavior

## Review Fix: Add min-w-0 to Truncating Spans

### Changes Made

**File: `src/views/workspace/PlanTracker.tsx`**
- Line 141: Changed `<span className="truncate">` to `<span className="min-w-0 truncate">`
- Line 167: Changed `<span className="truncate">` to `<span className="min-w-0 truncate">`

Rationale: The span is a flex item inside `ItemTitle`; `min-w-0` guarantees it can shrink below its content width, ensuring `text-overflow: ellipsis` renders correctly. Matches codebase convention (see `src/views/chat/ConversationList.tsx:304`).

### Testing & Verification

- **Unit tests:** `npx vitest run src/views/workspace/PlanTracker.test.tsx` — 11 passed ✓
- **Linting:** `npm run lint` (oxlint) — exit 0 ✓

### Commit

```
Commit: c679fed
Message: fix(chat): min-w-0 on truncating spans per review
Files: 1 changed, 2 insertions(+), 2 deletions(-)
```
