import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { Mail } from "lucide-react";
import ActivityCard from "./ActivityCard";

describe("ActivityCard", () => {
  it("draft: shows the body preview and Send / Edit / Discard actions", () => {
    render(
      <ActivityCard
        kind="draft"
        logo={<Mail />}
        title="Draft reply — Re: Q3 roadmap"
        meta="to Sarah Chen"
        provenance="Triage inbox"
        timestamp="4m"
        bodyPreview="Thanks Sarah — Thursday works."
      />,
    );

    expect(screen.getByText("Draft reply — Re: Q3 roadmap")).toBeInTheDocument();
    expect(screen.getByText("Thanks Sarah — Thursday works.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Send" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Edit" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Discard" })).toBeInTheDocument();
  });

  it("event: derives the Confirm label from the selected slot", () => {
    render(
      <ActivityCard
        kind="event"
        logo={<Mail />}
        title="Hold a slot for the design review?"
        timestamp="6m"
        slots={[
          { label: "Thu 2:00", selected: true },
          { label: "Thu 4:30" },
          { label: "Fri 11:00" },
        ]}
      />,
    );

    expect(screen.getByRole("button", { name: "Confirm Thu 2:00" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Other time" })).toBeInTheDocument();
    expect(screen.getAllByTestId("activity-card-slot")).toHaveLength(3);
    expect(screen.getByText("Thu 4:30")).toBeInTheDocument();
  });

  it("file: renders Open only when onOpen is provided, plus Dismiss", () => {
    const { rerender } = render(
      <ActivityCard kind="file" logo={<Mail />} title="Saved report.pdf" timestamp="1h" />,
    );
    expect(screen.queryByRole("button", { name: "Open" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Dismiss" })).toBeInTheDocument();

    rerender(
      <ActivityCard
        kind="file"
        logo={<Mail />}
        title="Saved report.pdf"
        timestamp="1h"
        onOpen={() => {}}
      />,
    );
    expect(screen.getByRole("button", { name: "Open" })).toBeInTheDocument();
  });

  it("shell: renders View log and Dismiss", () => {
    render(
      <ActivityCard
        kind="shell"
        logo={<Mail />}
        title="Upgraded 4 packages on your Mac"
        timestamp="1h"
        onViewLog={() => {}}
      />,
    );
    expect(screen.getByRole("button", { name: "View log" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Dismiss" })).toBeInTheDocument();
  });

  it("marks unread cards with an unread indicator", () => {
    render(<ActivityCard kind="draft" logo={<Mail />} title="Draft reply" timestamp="4m" unread />);
    expect(screen.getByTestId("activity-card-unread")).toBeInTheDocument();
  });
});
