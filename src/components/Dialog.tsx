import { useRef, type ReactNode } from "react"

import { Dialog as DialogRoot, DialogContent } from "@/components/ui/dialog"

export interface DialogProps {
  open: boolean
  onClose: () => void
  children: ReactNode
}

export default function Dialog({ open, onClose, children }: DialogProps) {
  const contentRef = useRef<HTMLDivElement>(null)

  return (
    <DialogRoot
      open={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen) {
          onClose()
        }
      }}
    >
      <DialogContent
        ref={contentRef}
        initialFocus={contentRef}
        showCloseButton={false}
        data-testid="app-dialog-content"
        className="w-[30rem] max-w-[90vw] overflow-hidden p-0"
      >
        {children}
      </DialogContent>
    </DialogRoot>
  )
}
