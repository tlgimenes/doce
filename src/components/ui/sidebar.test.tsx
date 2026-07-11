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

  it("toggles sidebar state via the Cmd/Ctrl+B keyboard shortcut", async () => {
    const user = userEvent.setup()

    render(
      <SidebarProvider defaultOpen={true}>
        <SidebarStateProbe />
      </SidebarProvider>
    )

    expect(screen.getByTestId("sidebar-state")).toHaveTextContent("expanded")

    await user.keyboard("{Meta>}b{/Meta}")

    expect(screen.getByTestId("sidebar-state")).toHaveTextContent("collapsed")

    await user.keyboard("{Control>}b{/Control}")

    expect(screen.getByTestId("sidebar-state")).toHaveTextContent("expanded")
  })

  it("toggles sidebar state and writes the state cookie", async () => {
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
    expect(cookieSetterSpy).toHaveBeenCalledWith("sidebar_state=false; path=/; max-age=604800")
  })
})
