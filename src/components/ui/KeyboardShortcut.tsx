import { cn } from "@/lib/cn";
import type { HTMLAttributes } from "react";

export interface KeyboardKeyProps {
  children: string;
  className?: string;
}

export interface KeyboardShortcutProps extends HTMLAttributes<HTMLSpanElement> {
  keys: string[];
  keyClassName?: string;
  separatorClassName?: string;
}

export function KeyboardKey({ children, className }: KeyboardKeyProps) {
  return (
    <kbd className={cn("rounded border border-border bg-muted px-1.5 py-0.5 font-mono text-xs", className)}>
      {children}
    </kbd>
  );
}

export function KeyboardShortcut({
  keys,
  className,
  keyClassName,
  separatorClassName,
  ...spanProps
}: KeyboardShortcutProps) {
  const visibleKeys = keys.filter((key) => key !== "");

  if (visibleKeys.length === 0) {
    return null;
  }

  return (
    <span
      {...spanProps}
      className={cn("inline-flex items-center gap-1", className)}
      aria-label={visibleKeys.join(" plus ")}
    >
      {visibleKeys.map((key, index) => (
        <span key={`${key}-${index}`} className="inline-flex items-center gap-1">
          <KeyboardKey className={keyClassName}>{key}</KeyboardKey>
          {index < visibleKeys.length - 1 && (
            <span className={cn("text-muted-foreground", separatorClassName)}>+</span>
          )}
        </span>
      ))}
    </span>
  );
}
