//! The one line that tells a user what an agent did while they were not looking: "Claude opened 3
//! terminals on rasp". The notification and the tab's trace say it the same way.

/// What `program` is called in a line: the agent's own name ("Claude") when the server's word for
/// the program is one the table of programs knows ([`crate::program::identity::from_server`] — the
/// one table, so a versioned install that runs as `2.1.296` or an agent under `node` is not
/// named after what the server happens to call the file); `None` for a shell, an unknown name
/// and an agent with no name of its own.
fn agent_name(program: Option<&str>) -> Option<String> {
    use crate::program::identity::{Kind, from_server};
    let name = program?.rsplit('/').next()?;
    match from_server(name)? {
        Kind::Agent(agent) => agent.title().map(str::to_owned),
        _ => None,
    }
}

/// [`agent_name`], and "A program" when it is none.
fn who(program: Option<&str>) -> String {
    agent_name(program).unwrap_or_else(|| "A program".to_owned())
}

/// Who asked for a pane, in the words a card says them: "Claude (t4)" when the program is
/// known, the terminal it runs in alone otherwise.
pub fn opener(program: Option<&str>, terminal: &str) -> String {
    let terminal: String = terminal.chars().filter(|c| !c.is_control()).collect();
    match agent_name(program) {
        Some(name) => format!("{name} ({terminal})"),
        None => terminal,
    }
}

/// "Claude opened 3 terminals on rasp": `program` is what ran in the terminal that asked (the
/// agent), `host` the server as the user typed it.
pub fn opened(program: Option<&str>, host: &str, count: u32) -> String {
    let host = bt_core::bare_host(host);
    let host: String = host.chars().filter(|c| !c.is_control()).collect();
    let things = if count == 1 {
        "a terminal".to_owned()
    } else {
        format!("{count} terminals")
    };
    format!("{} opened {things} on {host}", who(program))
}

/// What a notification an agent asked for (`bateri notify`) is titled: who said it and where,
/// "Claude on rasp". The words themselves are the body.
pub fn notified(program: Option<&str>, host: &str) -> String {
    let host = bt_core::bare_host(host);
    let host: String = host.chars().filter(|c| !c.is_control()).collect();
    format!("{} on {host}", who(program))
}

/// The question the user is asked when a program wants a terminal closed that it did not open:
/// the text, and the words under it. `caller` is the terminal the program runs in, when it runs
/// in one, and `program` what runs there.
pub fn close_question(
    program: Option<&str>,
    caller: Option<&str>,
    terminal: &str,
    host: &str,
) -> (String, String) {
    let host = bt_core::bare_host(host);
    let clean = |text: &str| -> String { text.chars().filter(|c| !c.is_control()).collect() };
    let (host, terminal) = (clean(host), clean(terminal));
    let who = match caller {
        Some(caller) => format!("{} ({})", who(program), clean(caller)),
        None => format!("A program on {host} that runs in no terminal"),
    };
    (
        format!("{who} wants to close {terminal} on {host}."),
        "What runs in it ends, and the terminal goes.".to_owned(),
    )
}

/// The words under [`opened`] in a notification: where they are, agreeing with how many.
pub fn found_in(count: u32) -> &'static str {
    if count == 1 {
        "It is in bateri."
    } else {
        "They are in bateri."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_is_named_by_the_table_of_programs_not_by_what_the_server_calls_the_file() {
        // A versioned install runs as its version; an agent under `node` is `node`.
        for program in ["2.1.296", "node", "python3", "zsh", "", "a b"] {
            assert_eq!(who(Some(program)), "A program", "{program}");
        }
        assert_eq!(opener(Some("2.1.296"), "t4"), "t4");
        assert_eq!(who(Some("claude-code")), "Claude");
        assert_eq!(who(Some("gemini")), "Gemini");
    }

    #[test]
    fn a_card_says_who_asked_for_a_pane() {
        assert_eq!(opener(Some("claude"), "t4"), "Claude (t4)");
        assert_eq!(opener(Some("/usr/bin/codex"), "t12"), "Codex (t12)");
        assert_eq!(opener(Some("zsh"), "t4"), "t4");
        assert_eq!(opener(None, "b-0a1b"), "b-0a1b");
        assert_eq!(opener(None, "t\u{1b}4"), "t4");
    }

    #[test]
    fn a_notification_an_agent_asked_for_says_who_and_where() {
        assert_eq!(notified(Some("claude"), "rasp"), "Claude on rasp");
        assert_eq!(notified(Some("zsh"), "ada@rasp"), "A program on rasp");
        assert_eq!(notified(None, "rasp\u{1b}[31m"), "A program on rasp[31m");
    }

    #[test]
    fn the_question_names_who_asks_what_for_and_where() {
        let (text, under) = close_question(Some("claude"), Some("t4"), "t7", "rasp");
        assert_eq!(text, "Claude (t4) wants to close t7 on rasp.");
        assert!(under.contains("ends"));
        let (text, _) = close_question(None, None, "t7", "rasp");
        assert_eq!(
            text,
            "A program on rasp that runs in no terminal wants to close t7 on rasp."
        );
        // Nothing from outside moves the user's screen.
        let (text, _) = close_question(Some("a\u{7}b"), Some("t\u{1b}4"), "t7", "r\u{0}asp");
        assert!(!text.chars().any(char::is_control), "{text:?}");
    }

    #[test]
    fn the_words_under_the_line_agree_with_the_number() {
        assert_eq!(found_in(1), "It is in bateri.");
        assert_eq!(found_in(2), "They are in bateri.");
    }

    #[test]
    fn the_line_names_the_agent_the_number_and_the_server() {
        assert_eq!(
            opened(Some("claude"), "rasp", 3),
            "Claude opened 3 terminals on rasp"
        );
        assert_eq!(
            opened(Some("/usr/local/bin/codex"), "ada@prod", 1),
            "Codex opened a terminal on prod"
        );
    }

    #[test]
    fn a_shell_or_a_name_nobody_reads_is_a_program() {
        for program in [
            None,
            Some("zsh"),
            Some("-bash"),
            Some(""),
            Some("a b"),
            Some("\x1b[31m"),
        ] {
            assert_eq!(
                opened(program, "rasp", 2),
                "A program opened 2 terminals on rasp",
                "{program:?}"
            );
        }
    }
}
