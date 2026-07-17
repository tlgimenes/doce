pub mod agent;
pub mod attachments;
pub mod context;
pub mod conversations;
pub mod mcp;
pub mod models;
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
            conversations::create_conversation,
            conversations::list_conversations,
            conversations::list_messages,
            conversations::mark_conversation_seen,
            conversations::archive_conversation,
            conversations::set_conversation_goal,
            conversations::get_conversation_goal,
            conversations::is_generation_active,
            conversations::stop_generation,
            context::get_context_usage,
            context::compact_conversation,
            search::search_conversations,
            settings::get_settings,
            settings::update_setting,
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
            agent::AskUserQuestionEvent,
            crate::context::ContextUsage,
            agent::AgentMessagePersisted,
            agent::AgentGenerationPiece,
            agent::PlanUpdate,
            agent::GoalComplete,
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

#[cfg(test)]
mod tests {
    /// Regenerates `../src/lib/bindings.ts` without launching the app —
    /// the exact export `lib::run` performs at debug startup. Run after
    /// changing the command/event surface:
    ///   cargo test --lib export_typescript_bindings -- --ignored
    #[test]
    #[ignore]
    fn export_typescript_bindings() {
        super::specta_builder()
            .export(
                specta_typescript::Typescript::default(),
                "../src/lib/bindings.ts",
            )
            .expect("failed to export typescript bindings");
    }
}
