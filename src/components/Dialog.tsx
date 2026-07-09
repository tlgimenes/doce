import { type ReactNode } from "react";
import { Dialog as DialogRoot, DialogContent } from "@/components/ui/dialog";

export interface DialogProps {
  open: boolean;
  onClose: () => void;
  children: ReactNode;
}

export default function Dialog({ open, onClose, children }: DialogProps) {
  return (
    <DialogRoot
      open={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen) onClose();
      }}
    >
      <DialogContent
        className="border-border bg-popover text-popover-foreground"
        data-testid="app-dialog-content"
      >
        {children}
      </DialogContent>
    </DialogRoot>
  );
}
