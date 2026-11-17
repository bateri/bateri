//! The agents bateri works with, and what each one is given: one table, one row per agent. Only
//! Claude Code for now, whose `Stop` hook is given the answer when a turn ends, and which learns the
//! `bateri` command from a skill.
//!
//! Nothing else in bateri knows an agent here. The message (`bateri message`), its carriage (the
//! app channel), its keeping (`bt_core::Session::set_program_reply`) and the row that draws it
//! take a text from a pane and say nothing of who wrote it; a row of this table is only what
//! turns it on for one agent: the setting that asks for it, the file the agent's wrapper
//! (`wrap/claude`) reads to start the agent with the hook, and the hook itself; and the skill, the
//! files of a folder the agent is given for the session alone, with the setting that asks for it.
//! A new agent is a row, not a code path.

use bt_core::Settings;

use super::identity::{Agent, Kind};

/// One agent that says what it last answered.
pub struct Replier {
    /// What the settings window calls it.
    pub name: &'static str,
    /// What its pane runs, as the tab's icon tells ([`Kind`]).
    pub kind: Kind,
    /// The file the agent's wrapper reads below `~/.local/share/bateri/shell/agents/`: its
    /// content is the hook, as the agent's `--settings` takes it, and its absence is "off".
    pub file: &'static str,
    /// The hook: what the agent runs when a turn ends. It reads the agent's JSON on standard
    /// input, takes the field the agent puts the answer in and hands it to `bateri message`; the
    /// agent does not wait for it (`async`) and a failure is its own business, so the turn is
    /// never held or marked by it.
    pub hook: &'static str,
    /// Whether the user asked for it ([`Settings::agents_claude_integration`] for Claude Code).
    pub wanted: fn(&Settings) -> bool,
    /// The folder below `~/.local/share/bateri/shell/agents/` that holds the skill, as the agent's
    /// `--plugin-dir` takes it.
    pub skill_dir: &'static str,
    /// The files of that folder: each a path in it and its content. They are written where the
    /// skill's setting is on and taken away where it is not.
    pub skill: &'static [(&'static str, &'static str)],
    /// The file of [`Replier::skill`] that says the skill is there: the wrapper gives the folder to
    /// the agent only where this file is.
    pub skill_file: &'static str,
    /// Whether the user asked for the skill ([`Settings::agents_claude_skill`] for Claude Code).
    pub skill_wanted: fn(&Settings) -> bool,
}

/// Claude Code's `Stop` hook: `last_assistant_message` is the answer of the turn that ended.
const CLAUDE_HOOK: &str = r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"bateri message --json-field last_assistant_message || true","async":true}]}]}}"#;

/// Claude Code's plugin: the manifest and the one skill (`bateri:bateri`) that teaches the
/// `bateri` command. The text is the app's, so it is the version of the `bateri` command that is
/// there (a server's is the one the app set up).
const CLAUDE_PLUGIN: &str = include_str!("../../../../assets/shell/agents/claude/plugin.json");
const CLAUDE_SKILL: &str = include_str!("../../../../assets/shell/agents/claude/SKILL.md");
const CLAUDE_SKILL_FILE: &str = "skills/bateri/SKILL.md";

/// The agents, in the order the settings page lists them.
pub const REPLIERS: [Replier; 1] = [Replier {
    name: "Claude Code",
    kind: Kind::Agent(Agent::ClaudeCode),
    file: "claude.json",
    hook: CLAUDE_HOOK,
    wanted: |settings| settings.agents_claude_integration,
    skill_dir: "claude-plugin",
    skill: &[
        (".claude-plugin/plugin.json", CLAUDE_PLUGIN),
        (CLAUDE_SKILL_FILE, CLAUDE_SKILL),
    ],
    skill_file: CLAUDE_SKILL_FILE,
    skill_wanted: |settings| settings.agents_claude_skill,
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
        let off = Settings {
            agents_claude_integration: false,
            ..Settings::default()
        };
        let on = Settings::default();
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
    fn a_skill_is_a_plugin_the_agent_loads_for_one_session_and_says_when_to_use_it() {
        for row in &REPLIERS {
            assert!(!row.skill_dir.is_empty() && !row.skill_dir.contains('/'));
            let manifest = row
                .skill
                .iter()
                .find(|(path, _)| *path == ".claude-plugin/plugin.json")
                .expect("a manifest")
                .1;
            // One JSON object that names the plugin (no JSON parser in this crate: the shape is
            // checked as text).
            let manifest = manifest.trim();
            assert!(
                manifest.starts_with('{') && manifest.ends_with('}'),
                "{manifest}"
            );
            assert!(manifest.contains(r#""name": "bateri""#), "{manifest}");
            let skill = row
                .skill
                .iter()
                .find(|(path, _)| *path == row.skill_file)
                .expect("the skill's file")
                .1;
            // The skill is told by its description, so the description must say when.
            let front = skill.strip_prefix("---\n").expect("front matter");
            let (front, body) = front.split_once("\n---\n").expect("front matter ends");
            assert!(front.lines().any(|line| line == "name: bateri"), "{front}");
            let description = front
                .lines()
                .find_map(|line| line.strip_prefix("description: "))
                .expect("a description");
            assert!(description.contains("`bateri` command"), "{description}");
            assert!(description.len() < 600, "it is in every session's context");
            // The body names the verbs the user's words come to and the one rule of `--`.
            for needle in [
                "bateri split",
                "bateri tab",
                "bateri ls",
                "bateri capture",
                "--keep-open",
                "--wait",
                "bateri --help",
            ] {
                assert!(body.contains(needle), "{needle}");
            }
            // No path of the text says where it lives: it is the app's and moves with it.
            assert!(!body.contains(".local/share"), "a place in the text");
            assert!(skill.len() < 8192, "{} bytes", skill.len());
        }
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
