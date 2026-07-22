import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands, type EndpointTestResult, type ModelState } from "@/lib/ipc";
import AddModelEndpoint, { hostFromUrl, inferEndpointKind } from "./AddModelEndpoint";

vi.mock("@/lib/ipc", () => ({
  commands: {
    testModelEndpoint: vi.fn(),
    selectEndpointModel: vi.fn(),
  },
}));

const savedState = { activeId: "endpoint:x" } as unknown as ModelState;

function testOk(models: string[]): EndpointTestResult {
  return { ok: true, models, error: null };
}

async function typeUrl(user: ReturnType<typeof userEvent.setup>, url: string) {
  const input = screen.getByTestId("endpoint-url-input");
  await user.clear(input);
  await user.type(input, url);
}

describe("AddModelEndpoint", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(commands.testModelEndpoint).mockResolvedValue(testOk(["gpt-oss", "qwen"]));
    vi.mocked(commands.selectEndpointModel).mockResolvedValue(savedState);
  });

  it("starts on Local server: no API key, caching on, and a local privacy note", () => {
    render(<AddModelEndpoint onCancel={vi.fn()} onSaved={vi.fn()} />);

    expect(screen.queryByTestId("endpoint-api-key-input")).not.toBeInTheDocument();
    expect(screen.getByTestId("endpoint-url-input")).toHaveAttribute(
      "placeholder",
      "http://localhost:8080/v1",
    );
    expect(screen.getByTestId("endpoint-privacy-note")).toHaveTextContent("Runs on this Mac.");
  });

  it("reshapes for Hosted API: key required, caching toggle hidden, host in the privacy note", async () => {
    const user = userEvent.setup();
    render(<AddModelEndpoint onCancel={vi.fn()} onSaved={vi.fn()} />);

    await user.click(screen.getByRole("button", { name: "Hosted API" }));
    await typeUrl(user, "https://openrouter.ai/api/v1");

    expect(screen.getByTestId("endpoint-api-key-input")).toBeInTheDocument();
    expect(screen.getByTestId("endpoint-privacy-note")).toHaveTextContent(
      "Requests go to openrouter.ai.",
    );

    await user.click(screen.getByTestId("endpoint-advanced-toggle"));
    expect(screen.queryByTestId("endpoint-cache-prompt-switch")).not.toBeInTheDocument();
  });

  it("marks the LAN key optional and keeps a network privacy note", async () => {
    const user = userEvent.setup();
    render(<AddModelEndpoint onCancel={vi.fn()} onSaved={vi.fn()} />);

    await user.click(screen.getByRole("button", { name: "LAN cluster" }));
    expect(screen.getByTestId("endpoint-api-key-input")).toBeInTheDocument();
    expect(screen.getByText("API key (optional)")).toBeVisible();
    expect(screen.getByTestId("endpoint-privacy-note")).toHaveTextContent("Stays on your network.");
  });

  it("reveals the model dropdown and connected count only after a successful Test", async () => {
    const user = userEvent.setup();
    render(<AddModelEndpoint onCancel={vi.fn()} onSaved={vi.fn()} />);

    await typeUrl(user, "http://localhost:8080/v1");
    expect(screen.queryByTestId("endpoint-model-field")).not.toBeInTheDocument();

    await user.click(screen.getByTestId("endpoint-test-button"));

    await waitFor(() =>
      expect(commands.testModelEndpoint).toHaveBeenCalledWith(
        "http://localhost:8080/v1",
        undefined,
      ),
    );
    expect(await screen.findByTestId("endpoint-test-status")).toHaveTextContent(
      "Connected — 2 models",
    );
    expect(screen.getByTestId("endpoint-model-select")).toBeInTheDocument();
  });

  it("shows the endpoint's error message when Test fails", async () => {
    vi.mocked(commands.testModelEndpoint).mockResolvedValue({
      ok: false,
      models: [],
      error: "Add an API key — this host requires one.",
    });
    const user = userEvent.setup();
    render(<AddModelEndpoint onCancel={vi.fn()} onSaved={vi.fn()} />);

    await typeUrl(user, "https://openrouter.ai/api/v1");
    await user.click(screen.getByTestId("endpoint-test-button"));

    expect(await screen.findByTestId("endpoint-test-error")).toHaveTextContent(
      "Add an API key — this host requires one.",
    );
    expect(screen.queryByTestId("endpoint-model-field")).not.toBeInTheDocument();
  });

  it("falls back to a free-text model when the endpoint lists none", async () => {
    vi.mocked(commands.testModelEndpoint).mockResolvedValue(testOk([]));
    const user = userEvent.setup();
    render(<AddModelEndpoint onCancel={vi.fn()} onSaved={vi.fn()} />);

    await typeUrl(user, "http://localhost:8080/v1");
    await user.click(screen.getByTestId("endpoint-test-button"));

    expect(await screen.findByTestId("endpoint-model-input")).toBeInTheDocument();
    expect(screen.queryByTestId("endpoint-model-select")).not.toBeInTheDocument();
  });

  it("saves a local endpoint with caching on, no key, and the default context window", async () => {
    const onSaved = vi.fn();
    const user = userEvent.setup();
    render(<AddModelEndpoint onCancel={vi.fn()} onSaved={onSaved} />);

    await typeUrl(user, "http://localhost:8080/v1");
    await user.click(screen.getByTestId("endpoint-test-button"));
    await screen.findByTestId("endpoint-model-select");
    await user.selectOptions(screen.getByTestId("endpoint-model-select"), "qwen");

    await user.click(screen.getByTestId("endpoint-save-button"));

    await waitFor(() =>
      expect(commands.selectEndpointModel).toHaveBeenCalledWith({
        kind: "local",
        url: "http://localhost:8080/v1",
        model: "qwen",
        apiKey: null,
        contextWindow: 32768,
        useCachePrompt: true,
      }),
    );
    expect(onSaved).toHaveBeenCalledWith(savedState);
  });

  it("saves a hosted endpoint with the entered key and no caching", async () => {
    const user = userEvent.setup();
    render(<AddModelEndpoint onCancel={vi.fn()} onSaved={vi.fn()} />);

    await user.click(screen.getByRole("button", { name: "Hosted API" }));
    await typeUrl(user, "https://openrouter.ai/api/v1");
    await user.type(screen.getByTestId("endpoint-api-key-input"), "sk-secret");
    await user.click(screen.getByTestId("endpoint-test-button"));
    await screen.findByTestId("endpoint-model-select");

    await user.click(screen.getByTestId("endpoint-save-button"));

    await waitFor(() =>
      expect(commands.selectEndpointModel).toHaveBeenCalledWith(
        expect.objectContaining({
          kind: "hosted",
          url: "https://openrouter.ai/api/v1",
          apiKey: "sk-secret",
          useCachePrompt: false,
        }),
      ),
    );
    // The Test probe passes the key too.
    expect(commands.testModelEndpoint).toHaveBeenCalledWith(
      "https://openrouter.ai/api/v1",
      "sk-secret",
    );
  });

  it("blocks Save for a hosted endpoint until a key is entered", async () => {
    const user = userEvent.setup();
    render(<AddModelEndpoint onCancel={vi.fn()} onSaved={vi.fn()} />);

    await user.click(screen.getByRole("button", { name: "Hosted API" }));
    await typeUrl(user, "https://openrouter.ai/api/v1");
    await user.click(screen.getByTestId("endpoint-test-button"));
    await screen.findByTestId("endpoint-model-select");

    expect(screen.getByTestId("endpoint-save-button")).toBeDisabled();
    await user.type(screen.getByTestId("endpoint-api-key-input"), "sk-1");
    expect(screen.getByTestId("endpoint-save-button")).toBeEnabled();
  });

  it("re-opens an existing endpoint pre-filled, warning that the saved key isn't shown", () => {
    render(
      <AddModelEndpoint
        prefill={{ kind: "hosted", url: "https://api.example.com/v1", model: "big-model" }}
        onCancel={vi.fn()}
        onSaved={vi.fn()}
      />,
    );

    expect(screen.getByTestId("endpoint-url-input")).toHaveValue("https://api.example.com/v1");
    expect(screen.getByText(/Reconnect model endpoint/)).toBeVisible();
    expect(screen.getByTestId("endpoint-api-key-input")).toBeInTheDocument();
    expect(screen.getByText(/saved key isn’t shown/)).toBeVisible();
  });

  it("switching kind clears a prior Test result", async () => {
    const user = userEvent.setup();
    render(<AddModelEndpoint onCancel={vi.fn()} onSaved={vi.fn()} />);

    await typeUrl(user, "http://localhost:8080/v1");
    await user.click(screen.getByTestId("endpoint-test-button"));
    await screen.findByTestId("endpoint-model-select");

    await user.click(screen.getByRole("button", { name: "Hosted API" }));
    expect(screen.queryByTestId("endpoint-model-field")).not.toBeInTheDocument();
    expect(screen.queryByTestId("endpoint-test-status")).not.toBeInTheDocument();
  });

  it("infers a starting kind and host from a URL", () => {
    expect(inferEndpointKind("http://localhost:8080/v1")).toBe("local");
    expect(inferEndpointKind("http://192.168.1.50:11434/v1")).toBe("lan");
    expect(inferEndpointKind("https://openrouter.ai/api/v1")).toBe("hosted");
    expect(hostFromUrl("https://openrouter.ai/api/v1")).toBe("openrouter.ai");
    expect(hostFromUrl("not a url")).toBeNull();
  });
});
