import { useEffect, useRef, type ReactNode } from "react";

export interface DialogProps {
  open: boolean;
  onClose: () => void;
  children: ReactNode;
}

export default function Dialog({ open, onClose, children }: DialogProps) {
  const ref = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;

    if (open && !dialog.open) {
      dialog.showModal();
      return;
    }

    if (!open && dialog.open) {
      dialog.close();
    }
  }, [open]);

  return (
    <dialog
      ref={ref}
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
      onClick={(event) => {
        if (event.target === ref.current) onClose();
      }}
      className="fixed left-1/2 top-1/2 z-50 w-[30rem] max-w-[90vw] -translate-x-1/2 -translate-y-1/2 overflow-hidden rounded-lg border border-border bg-card p-0 text-card-foreground backdrop:bg-black/40"
    >
      {open ? (
        <div className="contents" data-testid="app-dialog-content">
          {children}
        </div>
      ) : null}
    </dialog>
  );
}
