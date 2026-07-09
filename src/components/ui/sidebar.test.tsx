import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it, vi } from "vitest"

import { SidebarProvider, SidebarTrigger, useSidebar } from "./sidebar"

vi.mock("@/hooks/use-mobile", () => ({
  useIsMobile: () => false,
}))

function SidebarStateProbe() {
  const { state } = useSidebar()

  return <output data-testid="sidebar-state">{state}</output>
}

describe("SidebarProvider", () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it("does not register a global keydown listener", () => {
    const windowAddEventListenerSpy = vi.spyOn(window, "addEventListener")
    const documentAddEventListenerSpy = vi.spyOn(document, "addEventListener")

    render(
      <SidebarProvider>
        <SidebarStateProbe />
      </SidebarProvider>
    )

    expect(windowAddEventListenerSpy).not.toHaveBeenCalledWith(
      "keydown",
      expect.any(Function)
    )
    expect(documentAddEventListenerSpy).not.toHaveBeenCalledWith(
      "keydown",
      expect.any(Function)
    )
  })

  it("toggles sidebar state without writing document.cookie", async () => {
    const user = userEvent.setup()
    const cookieSetterSpy = vi.spyOn(Document.prototype, "cookie", "set")

    render(
      <SidebarProvider defaultOpen={true}>
        <SidebarStateProbe />
        <SidebarTrigger />
      </SidebarProvider>
    )

    expect(screen.getByTestId("sidebar-state")).toHaveTextContent("expanded")

    await user.click(screen.getByRole("button", { name: "Toggle Sidebar" }))

    expect(screen.getByTestId("sidebar-state")).toHaveTextContent("collapsed")
    expect(cookieSetterSpy).not.toHaveBeenCalled()
  })
})
