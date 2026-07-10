import { readdirSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const themePath = resolve(process.cwd(), "src/styles/theme.css");
const uiDirectoryPath = resolve(process.cwd(), "src/components/ui");
const srcRootPath = resolve(process.cwd(), "src");

const classicThemeAliases = [
  "background",
  "foreground",
  "card",
  "card-foreground",
  "popover",
  "popover-foreground",
  "primary",
  "primary-foreground",
  "secondary",
  "secondary-foreground",
  "muted",
  "muted-foreground",
  "accent",
  "accent-foreground",
  "destructive",
  "destructive-foreground",
  "border",
  "input",
  "ring",
  "sidebar",
  "sidebar-foreground",
  "sidebar-primary",
  "sidebar-primary-foreground",
  "sidebar-accent",
  "sidebar-accent-foreground",
  "sidebar-border",
  "sidebar-ring",
  "radius-sm",
  "radius-md",
  "radius-lg",
  "radius-xl",
] as const;

const ignoredRuntimeVariables = new Set([
  "anchor-width",
  "available-height",
  "available-width",
  "drawer-content-height",
  "drawer-content-max-height",
  "drawer-content-width",
  "drawer-frontmost-height",
  "drawer-height",
  "drawer-inset",
  "drawer-overlay-min-opacity",
  "drawer-snap-point-offset",
  "drawer-swipe-movement-x",
  "drawer-swipe-movement-y",
  "drawer-swipe-progress",
  "drawer-swipe-strength",
  "gap",
  "nested-drawers",
  "transform-origin",
]);

function collectFiles(directoryPath: string): string[] {
  const entries = readdirSync(directoryPath, { withFileTypes: true });

  return entries.flatMap((entry) => {
    if (entry.isDirectory()) {
      return collectFiles(resolve(directoryPath, entry.name));
    }

    return entry.name.endsWith(".ts") || entry.name.endsWith(".tsx")
      ? [resolve(directoryPath, entry.name)]
      : [];
  });
}

function collectDefinedVariables(source: string): Set<string> {
  return new Set(
    Array.from(source.matchAll(/["']?--([a-z0-9-]+)["']?\s*:/g), (match) => match[1]),
  );
}

function collectReferencedVariables(source: string): Set<string> {
  return new Set(Array.from(source.matchAll(/var\(--([a-z0-9-]+)\)/g), (match) => match[1]));
}

describe("theme.css compatibility bridge", () => {
  it("defines the classic shadcn alias set expected by generated primitives", () => {
    const themeSource = readFileSync(themePath, "utf8");
    const definedThemeVariables = collectDefinedVariables(themeSource);

    expect([...definedThemeVariables]).toEqual(
      expect.arrayContaining([...classicThemeAliases]),
    );
  });

  it("defines every non-local CSS variable referenced by source-owned UI primitives", () => {
    const themeSource = readFileSync(themePath, "utf8");
    const definedThemeVariables = collectDefinedVariables(themeSource);
    const missingReferences: Array<{ file: string; variable: string }> = [];

    for (const filePath of collectFiles(uiDirectoryPath)) {
      const source = readFileSync(filePath, "utf8");
      const locallyDefinedVariables = collectDefinedVariables(source);
      const referencedVariables = collectReferencedVariables(source);

      for (const variable of referencedVariables) {
        if (locallyDefinedVariables.has(variable) || ignoredRuntimeVariables.has(variable)) {
          continue;
        }

        if (!definedThemeVariables.has(variable)) {
          missingReferences.push({
            file: filePath.replace(srcRootPath, ""),
            variable,
          });
        }
      }
    }

    expect(missingReferences).toEqual([]);
  });
});
