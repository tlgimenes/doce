import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
  type MouseEvent,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { cn } from "@/lib/cn";

export type TopbarTarget = "sidebar" | "main";

type TopbarHosts = Partial<Record<TopbarTarget, HTMLDivElement>>;

interface TopbarContextValue {
  hosts: TopbarHosts;
  registerHost: (target: TopbarTarget, element: HTMLDivElement | null) => void;
}

const TopbarContext = createContext<TopbarContextValue | null>(null);

function useTopbarContext() {
  const context = useContext(TopbarContext);
  if (!context) {
    throw new Error("Topbar components must be rendered inside TopbarProvider");
  }
  return context;
}

export function TopbarProvider({ children }: { children: ReactNode }) {
  const [hosts, setHosts] = useState<TopbarHosts>({});

  const registerHost = useCallback((target: TopbarTarget, element: HTMLDivElement | null) => {
    setHosts((current) => {
      if (current[target] === element) return current;
      const next = { ...current };
      if (element) {
        next[target] = element;
      } else {
        delete next[target];
      }
      return next;
    });
  }, []);

  const value = useMemo(() => ({ hosts, registerHost }), [hosts, registerHost]);

  return <TopbarContext.Provider value={value}>{children}</TopbarContext.Provider>;
}

interface TopbarHostProps {
  target: TopbarTarget;
  className?: string;
  children?: ReactNode;
}

export function TopbarHost({ target, className, children }: TopbarHostProps) {
  const { registerHost } = useTopbarContext();

  const ref = useCallback(
    (element: HTMLDivElement | null) => {
      registerHost(target, element);
    },
    [registerHost, target],
  );

  const startDrag = async (event: MouseEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    const targetElement = event.target as HTMLElement | null;
    if (targetElement?.closest("[data-topbar-no-drag]")) return;

    event.preventDefault();
    await getCurrentWindow()
      .startDragging()
      .catch((error) => {
        console.error("Failed to start window dragging", error);
      });
  };

  return (
    <div
      ref={ref}
      className={cn(
        "flex h-10 shrink-0 select-none items-center bg-transparent",
        className,
      )}
      data-tauri-drag-region
      data-testid={`topbar-${target}`}
      onMouseDown={startDrag}
    >
      {children}
    </div>
  );
}

export function TopbarPortal({
  target,
  children,
}: {
  target: TopbarTarget;
  children: ReactNode;
}) {
  const { hosts } = useTopbarContext();
  const host = hosts[target];
  if (!host) return null;
  return createPortal(children, host);
}
