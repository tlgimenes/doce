import * as React from "react"
import { SearchIcon } from "lucide-react"

import { cn } from "@/lib/utils"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"

type CommandItemRecord = {
  id: string
  value: string
  keywords: string[]
}

const EMPTY_KEYWORDS: string[] = []

type CommandContextValue = {
  query: string
  setQuery: (value: string) => void
  shouldFilter: boolean
  registerItem: (item: CommandItemRecord) => void
  unregisterItem: (id: string) => void
  getVisibleItemCount: () => number
}

const CommandContext = React.createContext<CommandContextValue | null>(null)

function normalizeValue(value: string) {
  return value.trim().toLowerCase()
}

function getNodeText(node: React.ReactNode): string {
  if (typeof node === "string" || typeof node === "number") {
    return String(node)
  }

  if (Array.isArray(node)) {
    return node.map(getNodeText).join(" ")
  }

  if (React.isValidElement<{ children?: React.ReactNode }>(node)) {
    return getNodeText(node.props.children)
  }

  return ""
}

function matchesQuery(item: CommandItemRecord, query: string) {
  const normalizedQuery = normalizeValue(query)

  if (!normalizedQuery) {
    return true
  }

  const haystacks = [item.value, ...item.keywords]
  return haystacks.some((candidate) => normalizeValue(candidate).includes(normalizedQuery))
}

function useCommandContext() {
  return React.useContext(CommandContext)
}

function Command({
  className,
  value,
  onValueChange,
  shouldFilter = true,
  children,
  ...props
}: React.ComponentProps<"div"> & {
  value?: string
  onValueChange?: (value: string) => void
  shouldFilter?: boolean
}) {
  const [uncontrolledValue, setUncontrolledValue] = React.useState("")
  const [, setItemVersion] = React.useState(0)
  const itemsRef = React.useRef<Map<string, CommandItemRecord>>(new Map())

  const query = value ?? uncontrolledValue

  const setQuery = React.useCallback(
    (nextValue: string) => {
      onValueChange?.(nextValue)

      if (value === undefined) {
        setUncontrolledValue(nextValue)
      }
    },
    [onValueChange, value],
  )

  const registerItem = React.useCallback((item: CommandItemRecord) => {
    itemsRef.current.set(item.id, item)
    setItemVersion((current) => current + 1)
  }, [])

  const unregisterItem = React.useCallback((id: string) => {
    itemsRef.current.delete(id)
    setItemVersion((current) => current + 1)
  }, [])

  const getVisibleItemCount = React.useCallback(() => {
    if (!shouldFilter) {
      return itemsRef.current.size
    }

    let visibleCount = 0
    for (const item of itemsRef.current.values()) {
      if (matchesQuery(item, query)) {
        visibleCount += 1
      }
    }
    return visibleCount
  }, [query, shouldFilter])

  const contextValue = React.useMemo(
    () => ({
      query,
      setQuery,
      shouldFilter,
      registerItem,
      unregisterItem,
      getVisibleItemCount,
    }),
    [getVisibleItemCount, query, registerItem, setQuery, shouldFilter, unregisterItem],
  )

  return (
    <CommandContext.Provider value={contextValue}>
      <div
        data-slot="command"
        className={cn(
          "flex size-full flex-col overflow-hidden rounded-lg bg-popover p-1 text-popover-foreground",
          className,
        )}
        {...props}
      >
        {children}
      </div>
    </CommandContext.Provider>
  )
}

function CommandDialog({
  title = "Command Palette",
  description = "Search for a command to run...",
  children,
  className,
  showCloseButton = false,
  ...props
}: Omit<React.ComponentProps<typeof Dialog>, "children"> & {
  title?: string
  description?: string
  className?: string
  showCloseButton?: boolean
  children: React.ReactNode
}) {
  return (
    <Dialog {...props}>
      <DialogHeader className="sr-only">
        <DialogTitle>{title}</DialogTitle>
        <DialogDescription>{description}</DialogDescription>
      </DialogHeader>
      <DialogContent
        className={cn("top-1/3 translate-y-0 overflow-hidden p-0", className)}
        showCloseButton={showCloseButton}
      >
        {children}
      </DialogContent>
    </Dialog>
  )
}

function CommandInput({
  className,
  value,
  onValueChange,
  onChange,
  ...props
}: Omit<React.ComponentProps<"input">, "value"> & {
  value?: string
  onValueChange?: (value: string) => void
}) {
  const context = useCommandContext()
  const inputValue = value ?? context?.query ?? ""

  return (
    <div data-slot="command-input-wrapper" className="border-b px-3 py-2">
      <label className="flex items-center gap-2">
        <SearchIcon className="size-4 shrink-0 text-muted-foreground" />
        <input
          data-slot="command-input"
          className={cn(
            "flex h-8 w-full min-w-0 rounded-md bg-transparent text-sm outline-none placeholder:text-muted-foreground disabled:cursor-not-allowed disabled:opacity-50",
            className,
          )}
          value={inputValue}
          onChange={(event) => {
            context?.setQuery(event.target.value)
            onValueChange?.(event.target.value)
            onChange?.(event)
          }}
          {...props}
        />
      </label>
    </div>
  )
}

function CommandList({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="command-list"
      role="listbox"
      className={cn(
        "max-h-72 overflow-x-hidden overflow-y-auto outline-none",
        className,
      )}
      {...props}
    />
  )
}

function CommandEmpty({ className, ...props }: React.ComponentProps<"div">) {
  const context = useCommandContext()
  const isVisible = (context?.getVisibleItemCount() ?? 0) === 0

  if (!isVisible) {
    return null
  }

  return (
    <div
      data-slot="command-empty"
      className={cn("py-6 text-center text-sm text-muted-foreground", className)}
      {...props}
    />
  )
}

function CommandGroup({
  className,
  heading,
  children,
  ...props
}: React.ComponentProps<"div"> & {
  heading?: React.ReactNode
}) {
  return (
    <div
      data-slot="command-group"
      className={cn("overflow-hidden p-1 text-foreground", className)}
      {...props}
    >
      {heading ? (
        <div className="px-2 py-1.5 text-xs font-medium text-muted-foreground">
          {heading}
        </div>
      ) : null}
      {children}
    </div>
  )
}

function CommandItem({
  className,
  value,
  keywords = EMPTY_KEYWORDS,
  onSelect,
  children,
  ...props
}: Omit<React.ComponentProps<"button">, "value" | "onSelect"> & {
  value?: string
  keywords?: string[]
  onSelect?: (value: string) => void
}) {
  const context = useCommandContext()
  const id = React.useId()
  const registerItem = context?.registerItem
  const unregisterItem = context?.unregisterItem
  const resolvedValue = React.useMemo(
    () => (value && value.length > 0 ? value : getNodeText(children)),
    [children, value],
  )
  const keywordSignature = keywords.join("\u0000")
  const resolvedKeywords = React.useMemo(
    () => keywords.map((keyword) => keyword.trim()).filter((keyword) => keyword.length > 0),
    [keywordSignature],
  )

  React.useEffect(() => {
    if (!registerItem || !unregisterItem) {
      return
    }

    registerItem({ id, value: resolvedValue, keywords: resolvedKeywords })
    return () => unregisterItem(id)
  }, [id, registerItem, resolvedKeywords, resolvedValue, unregisterItem])

  const visible =
    !context?.shouldFilter ||
    matchesQuery({ id, value: resolvedValue, keywords: resolvedKeywords }, context.query)

  if (!visible) {
    return null
  }

  return (
    <button
      type="button"
      data-slot="command-item"
      role="option"
      className={cn(
        "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm outline-none transition-colors hover:bg-muted focus-visible:bg-muted disabled:pointer-events-none disabled:opacity-50 [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4",
        className,
      )}
      onClick={(event) => {
        props.onClick?.(event)
        if (!event.defaultPrevented) {
          onSelect?.(resolvedValue)
        }
      }}
      {...props}
    >
      {children}
    </button>
  )
}

function CommandSeparator({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="command-separator"
      role="separator"
      className={cn("-mx-1 my-1 h-px bg-border", className)}
      {...props}
    />
  )
}

function CommandShortcut({ className, ...props }: React.ComponentProps<"span">) {
  return (
    <span
      data-slot="command-shortcut"
      className={cn("ml-auto text-xs text-muted-foreground", className)}
      {...props}
    />
  )
}

export {
  Command,
  CommandDialog,
  CommandInput,
  CommandList,
  CommandEmpty,
  CommandGroup,
  CommandItem,
  CommandShortcut,
  CommandSeparator,
}
