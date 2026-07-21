import type { ComponentType, SVGProps } from "react";
import { Calendar, Mail, StickyNote } from "lucide-react";

/**
 * Icon glyphs for the activity feed. Service glyphs stay monochrome
 * (`stroke="currentColor"`) so they inherit the card's neutral ink and sit
 * inside the monochrome design system — the only exception is the Google
 * "G", a brand affordance reserved for the connect button + connection
 * chip. Gmail/Calendar/Keep reuse lucide primitives (envelope / calendar
 * grid / note); Drive has no lucide brand glyph, so it ships as a small
 * inline triangle here.
 */
export type ServiceIcon = ComponentType<{ className?: string; size?: number | string }>;

export const GmailIcon: ServiceIcon = Mail;
export const CalendarIcon: ServiceIcon = Calendar;
export const KeepIcon: ServiceIcon = StickyNote;

/** A monochrome Drive-style triangle (outline, inherits currentColor). */
export function DriveIcon({
  className,
  size = 16,
}: {
  className?: string;
  size?: number | string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.7}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <path d="M9 3h6l6 10.5H15z" />
      <path d="M9 3 3 13.5 6 19h6z" />
      <path d="M6 19h12l3-5.5H9z" />
    </svg>
  );
}

/**
 * The 4-color Google "G". The sole non-neutral, non-semantic mark allowed
 * in the feed — used only where Google's brand guidelines expect it (the
 * account connect button and the live connection chip).
 */
export function GoogleGIcon(props: SVGProps<SVGSVGElement> & { size?: number | string }) {
  const { size = 15, ...rest } = props;
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true" {...rest}>
      <path
        fill="#4285F4"
        d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92a5.06 5.06 0 0 1-2.2 3.32v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.1z"
      />
      <path
        fill="#34A853"
        d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84A11 11 0 0 0 12 23z"
      />
      <path fill="#FBBC05" d="M5.84 14.1a6.6 6.6 0 0 1 0-4.2V7.06H2.18a11 11 0 0 0 0 9.88z" />
      <path
        fill="#EA4335"
        d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.06L5.84 9.9C6.71 7.3 9.14 5.38 12 5.38z"
      />
    </svg>
  );
}
