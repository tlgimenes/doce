# Task 1 Report

## Outcome

Task 1 is complete in `/Users/gimenes/code/doce/.worktrees/shadcn-base-ui-redesign`.

## Audit Summary

- Confirmed the worktree already contained a partial shadcn Base UI bootstrap:
  `components.json`, generated `src/components/ui/*`, package manifest changes,
  and in-progress `Button` / `Dialog` / theme test updates.
- Audited the partial diff against the Task 1 brief and corrected the places
  where it still diverged materially.

## What Was Finished

- Kept the generated shadcn Base UI component layer under `src/components/ui`.
- Preserved the required app-facing `Button` contract:
  - native `button` root
  - `ButtonVariant = "primary" | "secondary" | "destructive" | "ghost"`
  - `ButtonSize = "sm" | "md" | "icon" | "icon-sm"`
  - `buttonVariants({ variant, size })`
- Preserved the app-facing `Dialog` wrapper contract while delegating to the
  generated shadcn dialog and keeping `data-testid="app-dialog-content"`.
- Replaced the theme token block in `src/styles/theme.css` with the Brand
  Accent Workbench values from the brief, while keeping the existing
  view-transition and reduced-motion sections.
- Kept `src/lib/utils.ts` as a delegation shim to the existing `cn()` helper.

## Additional Fixes Required To Reach Green

- Adjusted `src/components/ui/combobox.tsx` to stop using a generated
  button-polymorphism pattern that no longer works once `Button` is native-only.
- Removed an unused `React` import in `src/components/ui/scroll-area.tsx` that
  broke the TypeScript build under the repo's strict settings.
- Kept the Radix guard in `src/components/ui/button.test.tsx`, but used a
  repo-relative `readFileSync("src/components/ui/button.tsx", "utf8")` path
  instead of `new URL("./button.tsx", import.meta.url)` because Vitest in this
  repo does not expose a `file:` URL there at runtime.

## Verification

Ran successfully:

```bash
npm test -- src/components/ui/button.test.tsx src/components/Dialog.test.tsx
npm run build
```

Results:

- Targeted unit tests: 15 passed, 0 failed
- Production build: passed

## Notes

- No network escalation was required.
- `@radix-ui/react-slot` remains in the package manifest; source usage for Task 1
  was removed from `Button`, and later cleanup is already scoped separately in
  the broader redesign plan.

## Review Fix Follow-up

### Scope

- Removed the last direct Radix dependency and the unused generated `cmdk`
  command component that kept Radix packages in the lockfile.
- Tightened generated dialog radii in `src/components/ui/dialog.tsx` from
  `rounded-xl` / `rounded-b-xl` to `rounded-lg` / `rounded-b-lg`.
- Restored the app-level `src/components/Dialog.tsx` wrapper to the native
  `<dialog>` contract that the existing app and test setup depend on, while
  leaving the generated Base UI dialog foundation in place for Task 1.
- Updated focused tests so Task 1 coverage now checks the native dialog
  wrapper contract and the generated dialog radius rule.

### Additional Debugging Evidence

Before the wrapper fix, broader verification exposed Task 1 regressions in the
app-level dialog contract:

```bash
npm test -- src/App.test.tsx --testNamePattern="Cmd\+F opens conversation search in a dialog|Escape and the close button both dismiss the shortcuts dialog"
npm test -- src/views/chat/ConversationList.test.tsx --testNamePattern="renders sidebar actions at the top of the sidebar body while search opens in a dialog"
```

Results after the wrapper fix:

- `src/App.test.tsx`: 2 passed, 20 skipped
- `src/views/chat/ConversationList.test.tsx`: 1 passed, 13 skipped

### Required Verification

Ran successfully:

```bash
npm test -- src/components/ui/button.test.tsx src/components/Dialog.test.tsx
npm run build
npm test
npm run lint
rg "@radix-ui|radix-ui" src package.json package-lock.json
```

Results:

- Focused Task 1 tests: 17 passed, 0 failed
- Production build: passed
- Full test suite: 48 files passed, 356 tests passed
- Lint: passed
- Radix grep: no matches (`rg` exit code 1)
