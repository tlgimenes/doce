import Dialog from "@/components/Dialog";
import type { Conversation } from "@/lib/ipc";
import SearchPanel from "./SearchPanel";

interface ConversationSearchDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  recentConversations: Conversation[];
  onSelectConversationId: (conversationId: string) => void;
}

export default function ConversationSearchDialog({
  open,
  onOpenChange,
  recentConversations,
  onSelectConversationId,
}: ConversationSearchDialogProps) {
  return (
    <Dialog open={open} onClose={() => onOpenChange(false)}>
      <div data-testid="conversation-search-dialog">
        <SearchPanel
          recentConversations={recentConversations}
          onSelect={(conversationId) => {
            onSelectConversationId(conversationId);
            onOpenChange(false);
          }}
        />
      </div>
    </Dialog>
  );
}
