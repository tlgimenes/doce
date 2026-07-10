import { type ReactNode } from "react"
import { cn } from "@/lib/cn"

import {
  Dialog as DialogRoot,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"

export interface DialogProps {
  open: boolean
  onClose: () => void
  title: string
  description?: string
  contentClassName?: string
  children: ReactNode
}

export default function Dialog({
  open,
  onClose,
  title,
  description,
  contentClassName,
  children,
}: DialogProps) {
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
        showCloseButton={false}
        data-testid="app-dialog-content"
        className={cn("w-[30rem] max-w-[90vw] overflow-hidden p-0", contentClassName)}
      >
        <DialogHeader className="sr-only">
          <DialogTitle>{title}</DialogTitle>
          {description ? <DialogDescription>{description}</DialogDescription> : null}
        </DialogHeader>
        {children}
      </DialogContent>
    </DialogRoot>
  )
}
