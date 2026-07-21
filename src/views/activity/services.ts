import { CalendarIcon, DriveIcon, GmailIcon, KeepIcon, type ServiceIcon } from "./icons";

/**
 * The Google services the connect surface can grant. Kept as data (not
 * hardcoded JSX per service) so adding another Google tool is a one-line
 * entry, never new markup. `scope` is the plain-language capability caption
 * ("read + draft"), `toolCount` how many agent tools the grant unlocks.
 */
export interface GoogleService {
  id: string;
  name: string;
  description: string;
  icon: ServiceIcon;
  /** Plain-language capability caption, e.g. "read + draft". */
  scope: string;
  /** How many agent tools this grant unlocks. */
  toolCount: number;
}

export const GOOGLE_SERVICES: GoogleService[] = [
  {
    id: "gmail",
    name: "Gmail",
    description: "Triage your inbox and draft replies — reviewed by you before anything sends.",
    icon: GmailIcon,
    scope: "read + draft",
    toolCount: 4,
  },
  {
    id: "calendar",
    name: "Calendar",
    description: "Find free time and hold slots — events are only created once you confirm.",
    icon: CalendarIcon,
    scope: "read + propose",
    toolCount: 3,
  },
  {
    id: "keep",
    name: "Keep",
    description: "Capture notes and pull up lists the agent can read and add to.",
    icon: KeepIcon,
    scope: "read + write",
    toolCount: 2,
  },
  {
    id: "drive",
    name: "Drive",
    description: "Search your files and open the ones a task needs.",
    icon: DriveIcon,
    scope: "read only",
    toolCount: 3,
  },
];
