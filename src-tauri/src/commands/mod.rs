pub mod agent;
pub mod attachments;
pub mod context;
pub mod conversations;
pub mod mcp;
pub mod models;
pub mod scheduler;
pub mod search;
pub mod settings;
pub mod skills;
pub mod workspaces;

use tauri_specta::{collect_commands, collect_events, Builder};

pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            models::get_hardware_profile,
            models::start_model_install,
            models::get_model_install_status,
            models::list_models,
            models::set_active_model,
            conversations::create_conversation,
            conversations::send_message,
            conversations::list_conversations,
            conversations::list_messages,
            conversations::mark_conversation_seen,
            conversations::archive_conversation,
            conversations::is_generation_active,
            context::get_context_usage,
            context::compact_conversation,
            search::search_conversations,
            settings::get_settings,
            settings::update_setting,
            scheduler::set_focused_conversation,
            scheduler::cancel_generation,
            workspaces::open_workspace,
            workspaces::list_workspaces,
            workspaces::search_folders,
            agent::send_agent_message,
            agent::answer_user_question,
            agent::get_active_plan,
            attachments::read_attached_file,
            mcp::add_mcp_server,
            mcp::list_mcp_servers,
            mcp::list_mcp_server_tools,
            skills::list_skills,
        ])
        .events(collect_events![
            crate::downloader::ModelInstallProgress,
            conversations::AssistantToken,
            conversations::AssistantMessageComplete,
            conversations::AssistantMessageError,
            crate::scheduler::GenerationQueueUpdate,
            agent::AskUserQuestionEvent,
            crate::context::ContextUsage,
            agent::AgentMessagePersisted,
            agent::PlanUpdate,
        ])
        // Every timestamp field in this codebase is `i64` (Unix-epoch-ms,
        // per data-model.md's schema conventions) — specta-typescript
        // refuses to export 64-bit int types by default (a precision-loss
        // guard) unless this is enabled, mapping them to TS `bigint`
        // instead of `number`.
        .semantic_types(
            specta_typescript::semantic::Configuration::default().enable_lossless_bigints(),
        )
}
