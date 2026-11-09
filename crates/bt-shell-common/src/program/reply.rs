//! The agents that say what they last answered, and what each one needs for it: one table, one row
//! per agent. Only Claude Code for now, whose `Stop` hook is given the answer when a turn ends.
//!
//! Nothing else in bateri knows an agent here. The message (`bateri message`), its carriage (the
//! app channel), its keeping (`bt_core::Session::set_program_reply`) and the row that draws it
//! take a text from a pane and say nothing of who wrote it; a row of this table is only what
//! turns it on for one agent: the setting that asks for it, the file the shell function reads
//! to start the agent with the hook, and the hook itself. A new agent is a row, not a code path.

use bt_core::Settings;

use super::identity::{Agent, Kind};

/// One agent that says what it last answered.
pub struct Replier {
    /// What the settings window calls it.
    pub name: &'static str,
    /// What its pane runs, as the tab's icon tells ([`Kind`]).
    pub kind: Kind,
    /// The file the shell function reads below `~/.local/share/bateri/shell/agents/`: its
    /// content is the hook, as the agent's `--settings` takes it, and its absence is "off".
    pub file: &'static str,
    /// The hook: what the agent runs when a turn ends. It reads the agent's JSON on standard
    /// input, takes the field the agent puts the answer in and hands it to `bateri message`; the
    /// agent does not wait for it (`async`) and a failure is its own business, so the turn is
    /// never held or marked by it.
    pub hook: &'static str,
    /// Whether the user asked for it ([`Settings::agents_claude_last_reply`] for Claude Code).
    pub wanted: fn(&Settings) -> bool,
}

/// Claude Code's `Stop` hook: `last_assistant_message` is the answer of the turn that ended.
const CLAUDE_HOOK: &str = r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"bateri message --json-field last_assistant_message || true","async":true}]}]}}"#;

/// The agents, in the order the settings page lists them.
pub const REPLIERS: [Replier; 1] = [Replier {
    name: "Claude Code",
    kind: Kind::Agent(Agent::ClaudeCode),
    file: "claude.json",
    hook: CLAUDE_HOOK,
    wanted: |settings| settings.agents_claude_last_reply,
}];

/// The row of the agent a pane runs, if it is one that says what it last answered.
pub fn replier(kind: Kind) -> Option<&'static Replier> {
    REPLIERS.iter().find(|row| row.kind == kind)
}

/// Whether the last reply of an agent that runs as `kind` shows: it is in the table and the user
/// asked for it.
pub fn shows_reply(settings: &Settings, kind: Kind) -> bool {
    replier(kind).is_some_and(|row| (row.wanted)(settings))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_agent_in_the_table_that_was_asked_for_shows_its_reply() {
        let off = Settings::default();
        let on = Settings {
            agents_claude_last_reply: true,
            ..Settings::default()
        };
        let claude = Kind::Agent(Agent::ClaudeCode);
        assert!(!shows_reply(&off, claude));
        assert!(shows_reply(&on, claude));
        for other in [
            Kind::Agent(Agent::Codex),
            Kind::Agent(Agent::Other),
            Kind::Shell,
            Kind::Editor,
        ] {
            assert!(!shows_reply(&on, other), "{other:?}");
        }
        assert_eq!(replier(claude).map(|row| row.name), Some("Claude Code"));
    }

    #[test]
    fn a_hook_is_one_json_object_that_runs_the_message_verb_without_holding_the_agent() {
        for row in &REPLIERS {
            assert!(
                row.file.ends_with(".json") && !row.file.contains('/'),
                "{}",
                row.file
            );
            // Valid for the agent's `--settings`, and one argument for the shell function: no
            // single quote, which the function would have to escape.
            assert!(!row.hook.contains('\''), "{}", row.hook);
            assert!(row.hook.starts_with('{') && row.hook.ends_with('}'));
            assert!(row.hook.contains(r#""async":true"#), "{}", row.hook);
            assert!(
                row.hook.contains("bateri message --json-field "),
                "{}",
                row.hook
            );
            assert!(row.hook.contains("|| true"), "{}", row.hook);
            assert!(row.hook.len() < 4096, "an item of the files script");
        }
    }
}
