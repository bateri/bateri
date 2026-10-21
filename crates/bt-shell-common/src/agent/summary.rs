//! The one line that tells a user what an agent did while they were not looking: "Claude opened 3
//! terminals on rasp". The notification and the tab's trace say it the same way.

/// What `program` is called in the line: the agent's own name with a capital (`claude` →
/// "Claude"), and "A program" when it is a shell, unknown, or not a name a person reads.
fn who(program: Option<&str>) -> String {
    let name = program
        .map(|program| program.rsplit('/').next().unwrap_or(program))
        .filter(|name| {
            !name.is_empty()
                && name.len() <= 32
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-'))
                && !crate::program::is_shell_name(name)
        });
    match name {
        Some(name) => {
            let mut chars = name.chars();
            let first = chars.next().map(|c| c.to_ascii_uppercase());
            first.into_iter().chain(chars).collect()
        }
        None => "A program".to_owned(),
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
