import { useEffect, useRef, type ReactNode } from "react";

export interface DialogProps {
  open: boolean;
  onClose: () => void;
  children: ReactNode;
}

// The app's first modal (005-keyboard-shortcuts). Built directly on the
// native <dialog> element rather than a hand-rolled overlay or a library:
// WebKit's .showModal() gives focus-trapping, Escape-to-close (the native
// `cancel` event), and correct modal semantics for free (research.md § 2).
export default function Dialog({ open, onClose, children }: DialogProps) {
  const ref = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) {
      dialog.showModal();
    } else if (!open && dialog.open) {
      dialog.close();
    }
  }, [open]);

  return (
    <dialog
      ref={ref}
      onCancel={(e) => {
        // Escape fires the native `cancel` event — let the dialog element
        // handle its own closing, we just need to sync React state.
        e.preventDefault();
        onClose();
      }}
      onClick={(e) => {
        // Standard backdrop-click-to-close pattern for <dialog>: a click
        // that lands on the <dialog> element itself (the backdrop, since
        // its content has its own box) closes it; a click inside the
        // content does not, because the event target there is a
        // descendant, not the dialog element.
        if (e.target === ref.current) onClose();
      }}
      className="rounded-lg border border-border bg-card p-0 text-card-foreground backdrop:bg-black/40"
    >
      {/* Only mounted while open: a closed native <dialog> keeps its own
          box invisible via UA styles, but its content would otherwise stay
          in the DOM regardless — no reason to keep it there once closed,
          for this app's simple, stateless dialog content. */}
      {open && children}
    </dialog>
  );
}
