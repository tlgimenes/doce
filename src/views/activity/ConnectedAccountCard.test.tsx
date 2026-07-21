import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import ConnectedAccountCard from "./ConnectedAccountCard";
import { GOOGLE_SERVICES } from "./services";

describe("ConnectedAccountCard", () => {
  it("shows the account email and lists each granted service as a static row", () => {
    render(<ConnectedAccountCard email="sam@example.com" services={GOOGLE_SERVICES} />);

    expect(screen.getByText("sam@example.com")).toBeInTheDocument();
    expect(screen.getAllByTestId("granted-service-row")).toHaveLength(GOOGLE_SERVICES.length);
    expect(screen.getByText("Gmail")).toBeInTheDocument();
    expect(screen.getByText("Calendar")).toBeInTheDocument();
    expect(screen.getByText("Keep")).toBeInTheDocument();
    expect(screen.getByText("Drive")).toBeInTheDocument();
  });

  it("shows scope caption and tool count per service, not a toggle", () => {
    render(<ConnectedAccountCard email="sam@example.com" services={GOOGLE_SERVICES} />);

    expect(screen.getByText("read + draft")).toBeInTheDocument();
    expect(screen.getByText("4 tools")).toBeInTheDocument();
  });

  it("has NO per-service enable/disable toggles — only a single disconnect action", () => {
    render(<ConnectedAccountCard email="sam@example.com" services={GOOGLE_SERVICES} />);

    // The whole point of the connected card: once a service is connected it
    // is always available, so there are no switches anywhere.
    expect(screen.queryAllByRole("switch")).toHaveLength(0);
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Disconnect account" })).toBeInTheDocument();
  });
});
