# Feature Specification: Doce Landing Page

**Feature Branch**: `002-landing-page`

**Created**: 2026-07-03

**Status**: Draft

**Input**: User description: "Create a landing page for the Doce project hosted on GitHub Pages. Goals: bring visitors in and explain what Doce is and why it matters (fully local, zero-config personal AI agent for macOS, no API keys, no cloud dependency); provide a clear download link/button to get the app; display live GitHub stars count for the repo; include a 'Buy Me a Coffee' support button/link. This is a marketing/informational static site, separate from the core macOS app feature (001-doce-v1-core)."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Learn about Doce and download it (Priority: P1)

A person who has never heard of Doce arrives at the landing page (from a
link, search, or the GitHub repo). Within seconds they understand that Doce
is a fully local, zero-config personal AI agent for macOS, with no API keys
and no cloud dependency, and can click a clearly visible button to download
it.

**Why this priority**: This is the entire purpose of the page — converting a
visitor's attention into an understanding of the product and a download.
Every other element on the page is secondary to this journey.

**Independent Test**: Can be fully tested by loading the page in a fresh
browser session and confirming that, without scrolling, a visitor can read
the value proposition and locate the download call-to-action.

**Acceptance Scenarios**:

1. **Given** a visitor with no prior knowledge of Doce lands on the page,
   **When** the page finishes loading, **Then** they see, within the first
   screen of content, a clear statement that Doce is a free, fully local,
   zero-config personal AI agent for macOS.
2. **Given** the visitor wants to try Doce, **When** they click the primary
   download button, **Then** they are taken to the location that hosts the
   current build/release of the app.
3. **Given** a visitor on a device that is not a macOS/Apple Silicon machine,
   **When** they view the page, **Then** the platform requirement is stated
   clearly enough that they understand, before attempting to download,
   whether Doce will run on their machine.

---

### User Story 2 - Gauge project credibility via GitHub stars (Priority: P2)

A visitor deciding whether to trust or adopt Doce wants a quick signal of
community traction. The landing page shows the current GitHub star count for
the project's repository.

**Why this priority**: Social proof increases confidence and conversion, but
the page still fulfills its core purpose (explain + download) without it.

**Independent Test**: Can be fully tested by loading the page and confirming
a star count is displayed and reflects the repository's actual count within
a reasonable refresh window.

**Acceptance Scenarios**:

1. **Given** the visitor is on the landing page, **When** the page finishes
   loading, **Then** a GitHub star count for the Doce repository is visible.
2. **Given** the star count cannot be retrieved (e.g. the data source is
   temporarily unreachable), **When** the page loads, **Then** the rest of
   the page still renders correctly with no broken or error element in place
   of the count.

---

### User Story 3 - Support the project financially (Priority: P3)

A visitor who wants to support Doce's development — independent of whether
they use the app — can click a "Buy Me a Coffee" button to make a small
contribution.

**Why this priority**: A nice-to-have support channel; it does not affect
the page's primary conversion goal (helping visitors understand and download
Doce).

**Independent Test**: Can be fully tested by confirming a clearly labeled
"Buy Me a Coffee" button/link is present and opens a valid contribution page.

**Acceptance Scenarios**:

1. **Given** the visitor is on the landing page, **When** they look for a way
   to support the project, **Then** a clearly labeled "Buy Me a Coffee"
   button is visible and, when clicked, opens the corresponding support page.

---

### Edge Cases

- What happens when the star-count data source is rate-limited or
  unreachable? The page must show a graceful fallback (e.g. a cached/last
  known count, or hide the element) rather than an error state or blank gap.
- What happens when a visitor is not on macOS/Apple Silicon? They must still
  be able to read about the project without being misled into downloading a
  build that will not run on their machine.
- What happens before a public release/build exists to link to? The download
  button must degrade gracefully (e.g. link to a releases page, or show a
  clear "coming soon" state) rather than leading to a broken link.
- What happens on very small (mobile) or very large screens? Layout and the
  primary call-to-action must remain usable and visible without horizontal
  scrolling or overlapping content.
- What happens if a visitor has JavaScript disabled? Core content (value
  proposition, download link, coffee link) must still be readable and
  usable; only the live star count may be degraded.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The page MUST present, within the first screen of content, a
  concise explanation of what Doce is and its core value proposition (fully
  local, zero-config personal AI agent for macOS, no API keys, no cloud
  dependency).
- **FR-002**: The page MUST provide a single, visually prominent primary
  call-to-action that lets visitors download the current build of Doce.
- **FR-003**: The page MUST display the current star count of the Doce
  GitHub repository, refreshed at least once every 24 hours.
- **FR-004**: The page MUST provide a clearly labeled "Buy Me a Coffee"
  button/link that takes visitors to a valid external contribution page.
- **FR-005**: The page MUST be published via GitHub Pages and be publicly
  reachable without login, account creation, or any form submission.
- **FR-006**: The page MUST state the supported platform (macOS, Apple
  Silicon) clearly enough that a visitor knows, before downloading, whether
  Doce will run on their machine.
- **FR-007**: The page MUST remain fully readable and navigable if the
  GitHub star count cannot be retrieved, with no visible error state or
  broken layout in its place.
- **FR-008**: The page MUST be usable on both desktop and mobile browser
  viewport sizes without horizontal scrolling or overlapping content.
- **FR-009**: The page MUST NOT require visitors to create an account, sign
  in, or provide personal data to view content, download the app, or reach
  the support page.
- **FR-010**: The page MUST NOT include third-party visitor tracking or
  analytics beyond, at most, basic and privacy-respecting traffic counts,
  consistent with the project's local-first privacy stance.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A first-time visitor can identify what Doce is and locate the
  download call-to-action within 10 seconds of the page loading, without
  scrolling past the first screen.
- **SC-002**: The download call-to-action and the platform requirement
  (macOS/Apple Silicon) are both visible in the page's first screen of
  content on standard desktop and mobile screen sizes.
- **SC-003**: The displayed GitHub star count matches the repository's
  actual count within 24 hours, verified on demand.
- **SC-004**: When the star count cannot be fetched, 100% of test page loads
  still render a complete, unbroken layout.
- **SC-005**: A visitor can go from landing on the page to reaching the
  app's download location, or the "Buy Me a Coffee" page, in no more than 2
  clicks.
- **SC-006**: The page's primary content (value proposition and
  calls-to-action) is visible to a visitor within 2 seconds on a typical
  broadband connection.

## Assumptions

- No packaged release/build of Doce exists yet at the time of writing; the
  download button will point to the project's GitHub Releases page and will
  resolve to the latest published build once one exists.
- The page is hosted at the repository's default GitHub Pages URL; a custom
  domain is out of scope for this version of the feature.
- The GitHub star count is sourced from GitHub's own public repository data,
  and a refresh cadence of once daily is sufficient — sub-hour real-time
  updates are not required.
- The landing page is a single scrollable page (not a multi-page site) for
  this version.
- English is the only language required for this version of the page.
- "Buy Me a Coffee" refers to the buymeacoffee.com service (or equivalent);
  the specific account/page will be supplied during implementation.
