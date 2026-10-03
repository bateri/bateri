//! Dragged file paths and the clipboard text of Edit ▸ Paste Escaped Text → text that can be
//! written to the shell. **Pure and AppKit-free**, and therefore testable.
//!
//! Not inside `keys.rs` but its **sibling**: that module's header says "keystroke → PTY bytes",
//! and the question here is not a key but a drop or a clipboard. Its two consumers are
//! `view::BateriView`'s `performDragOperation:` ([`shell_quote`]) and `pasteEscaped:`
//! ([`paste_quote`]); in both the output goes to `Session::paste`, so the bracketed paste
//! wrapping and the dock exception are not this module's concern.

use std::fmt::Write as _;

/// Escapes the paths with backslashes and joins them with **a single space** — Terminal.app
/// parity (`/Users/…/İki\ Kelime/a.txt`).
///
/// **The escaped set is not a blacklist but the complement of a whitelist:** what passes is
/// ASCII letters/digits plus `/ . _ -`, and **every non-ASCII character**; every other ASCII
/// character is escaped. The opposite decision (enumerating the shell's metacharacters one by
/// one) would open up the debate of in which shell and where in a word `~`, `=`, `#`, `!`, `%`
/// are special, and a single character missing from the list would turn into a silent bug.
/// Escaping too much costs nothing (`\+` is `+` in the shell), so the error falls on the safe
/// side.
///
/// A non-ASCII character passes **untouched**: the space in `İki Kelime` is escaped, `İ` is
/// not — the shell already treats it as a plain letter and putting a backslash before it would
/// only be ugly.
///
/// **The known limit of the newline is stated by name:** `\` + newline is a *line
/// continuation* in both zsh and bash, so a file whose name carries a newline (pathological
/// but possible) is written with its two parts joined. Not escaping would be worse — a raw
/// newline becomes a command boundary in the buffer and would run a line the user did not
/// type. `$'\n'` would be correct but would split the single rule in two (one
/// type, one rule).
///
/// An empty list gives an empty string: if the drop has no readable path, there is nothing to
/// write either.
pub fn shell_quote(paths: &[String]) -> String {
    let mut line = String::new();
    for path in paths {
        if !line.is_empty() {
            line.push(' ');
        }
        for c in path.chars() {
            if needs_escape(c) {
                line.push('\\');
            }
            line.push(c);
        }
    }
    line
}

/// The rule of Edit ▸ Paste Escaped Text (⌃⌘V): text without a newline goes
/// through the Finder drop's escaping ([`shell_quote`], same appearance); text with a newline
/// is wrapped **entirely in single quotes** and any `'` inside becomes `'\''`.
///
/// Two branches, because the limit [`shell_quote`] accepts as "pathological" — `\` + newline
/// is a line continuation in the shell, the parts get joined — is ordinary in clipboard text.
/// A POSIX single quote carries the newline literally, and since the line does not end until
/// the quote closes, no line runs by itself even without bracketed wrapping. On a single line
/// the backslash is preferred: the same appearance as the drop, and quotes mean more
/// characters on a line the user edits afterwards.
///
/// A newline is `\n` **or** `\r` (the clipboard may carry `\r\n` from Windows): both are a
/// command boundary in the shell. Inside the quote `\r\n` and a lone `\r` are reduced to
/// `\n`, because zsh's bracketed reader turns every `\r` into `\n` and `\r\n` would become
/// two newlines per line — let the text stay the same line by line. Empty text gives an empty
/// string.
pub fn paste_quote(text: &str) -> String {
    if !text.contains(['\n', '\r']) {
        return shell_quote(&[text.to_owned()]);
    }
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('\'');
    for c in text.chars() {
        if c == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(c);
        }
    }
    quoted.push('\'');
    quoted
}

/// The remote session's **re-run line**: argv → a single readable line to be
/// written to the shell. The line written by ⌘T and by reconnecting is in front of the user's
/// eyes (the new tab's dock, the history), so [`shell_quote`]'s narrow whitelist for drops
/// would read `ssh deploy\@prod -o User\=x` here.
///
/// The rule is [`shell_quote`]'s, with three extensions: `@ : , +` pass everywhere (they are
/// not special in the shell), `=` passes **except at the start of a word** — zsh's `EQUALS`
/// expands `=cmd` only at the start of a word —, and an **empty argument** becomes `''`,
/// otherwise it would vanish from the line without a trace. An argument carrying a control
/// character goes into `$'…'`. Arguments are joined with a single space. The drop's rule does
/// not change: a separate entry point.
pub fn command_line(argv: &[String]) -> String {
    let mut line = String::new();
    for arg in argv {
        if !line.is_empty() {
            line.push(' ');
        }
        if arg.is_empty() {
            line.push_str("''");
            continue;
        }
        // A control character (newline first of all) cannot be escaped with a backslash: `\` +
        // newline is a line continuation in the shell and would join the argument, ESC would
        // go raw. That argument goes entirely into ANSI-C quotes (`$'…'`, zsh and bash) and
        // the character is written with a readable escape.
        if arg.chars().any(char::is_control) {
            line.push_str("$'");
            for c in arg.chars() {
                match c {
                    '\\' => line.push_str("\\\\"),
                    '\'' => line.push_str("\\'"),
                    '\n' => line.push_str("\\n"),
                    '\t' => line.push_str("\\t"),
                    '\r' => line.push_str("\\r"),
                    c if c.is_control() => {
                        let _ = write!(line, "\\u{:04x}", u32::from(c));
                    }
                    c => line.push(c),
                }
            }
            line.push('\'');
            continue;
        }
        for (at, c) in arg.chars().enumerate() {
            let readable = matches!(c, '@' | ':' | ',' | '+') || (c == '=' && at > 0);
            if needs_escape(c) && !readable {
                line.push('\\');
            }
            line.push(c);
        }
    }
    line
}

/// The whitelist itself: ASCII letters/digits and four punctuation marks pass, everything
/// non-ASCII passes, the remaining ASCII is escaped.
///
/// The reason for the four is the path's own vocabulary: `/` the separator, `.` the extension
/// and `..`, `_` and `-` the ordinary punctuation of file names. The question to ask when
/// adding a fifth is not "is it special in the shell" but "does it break if escaped" —
/// escaping is harmless, so the list should stay short.
fn needs_escape(c: char) -> bool {
    c.is_ascii() && !(c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quote(path: &str) -> String {
        shell_quote(&[path.to_owned()])
    }

    #[test]
    fn plain_paths_pass_through_untouched() {
        // A path with no character to escape goes as is: the whitelist is letters/digits and
        // the path's own punctuation.
        assert_eq!(quote("/Users/kalaomer/a.txt"), "/Users/kalaomer/a.txt");
        assert_eq!(quote("/tmp/bir_iki-uc.tar.gz"), "/tmp/bir_iki-uc.tar.gz");
    }

    #[test]
    fn spaces_are_escaped_so_the_shell_sees_one_argument() {
        // The headline case (Terminal.app parity): a file with a space in
        // its name must be a single argument, otherwise the shell sees two paths.
        assert_eq!(
            quote("/Users/a/İki Kelime/a.txt"),
            "/Users/a/İki\\ Kelime/a.txt"
        );
    }

    #[test]
    fn non_ascii_characters_are_not_escaped() {
        // Turkish letters are plain letters in the shell: putting a backslash before them
        // would only be ugly. The only thing escaped is the space in between (above).
        assert_eq!(quote("/tmp/ğüşİÖÇ.txt"), "/tmp/ğüşİÖÇ.txt");
        assert_eq!(quote("/tmp/日本語"), "/tmp/日本語");
    }

    #[test]
    fn shell_metacharacters_are_escaped() {
        // Where the blacklist debate is closed: the shell's metacharacters are not enumerated
        // one by one, they are escaped because they fall outside the whitelist.
        assert_eq!(quote("/tmp/$HOME"), "/tmp/\\$HOME");
        assert_eq!(quote("/tmp/`x`"), "/tmp/\\`x\\`");
        assert_eq!(quote("/tmp/a;b"), "/tmp/a\\;b");
        assert_eq!(quote("/tmp/a&b"), "/tmp/a\\&b");
        assert_eq!(quote("/tmp/a|b"), "/tmp/a\\|b");
        assert_eq!(quote("/tmp/a'b"), "/tmp/a\\'b");
        assert_eq!(quote("/tmp/a\"b"), "/tmp/a\\\"b");
        assert_eq!(quote("/tmp/a\\b"), "/tmp/a\\\\b");
        assert_eq!(quote("/tmp/a*b?c[d]"), "/tmp/a\\*b\\?c\\[d\\]");
        // The ones that would be up for debate without a whitelist: `~` is special only at
        // the start of a word, `=` only in zsh, `#` only at the start of a word, `!` only in
        // an interactive shell. All of them are escaped and escaping them is harmless.
        assert_eq!(quote("/tmp/~=#!%"), "/tmp/\\~\\=\\#\\!\\%");
    }

    #[test]
    fn tab_and_newline_are_escaped_with_a_backslash() {
        // Tab is a word separator, newline a command boundary: both must be escaped. The
        // **known limit** of the newline is nailed down as a contract — `\` + newline is a
        // line continuation in the shell, so the two parts of the name get joined. The cost of
        // not escaping is heavier (it would run a line the user did not type) and `$'\n'`
        // would split the single rule in two.
        assert_eq!(quote("/tmp/a\tb"), "/tmp/a\\\tb");
        assert_eq!(quote("/tmp/a\nb"), "/tmp/a\\\nb");
    }

    #[test]
    fn several_paths_are_joined_by_a_single_space() {
        // A multi-file drop: each path is escaped separately, with a **single** space between
        // them and no trailing space — we do not add an extra character to the line the user
        // is typing.
        assert_eq!(
            shell_quote(&["/tmp/a b".to_owned(), "/tmp/c".to_owned()]),
            "/tmp/a\\ b /tmp/c"
        );
        // An empty drop gives an empty string: no readable path, nothing to write.
        assert_eq!(shell_quote(&[]), "");
    }

    fn line(argv: &[&str]) -> String {
        command_line(&argv.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
    }

    #[test]
    fn a_command_line_stays_readable() {
        // `@ : , +` and a `=` inside a word are not escaped.
        assert_eq!(
            line(&["ssh", "-o", "User=x", "deploy@prod"]),
            "ssh -o User=x deploy@prod"
        );
        assert_eq!(
            line(&["ssh", "-J", "jump:2222,bastion", "-p", "2222", "host+1"]),
            "ssh -J jump:2222,bastion -p 2222 host+1"
        );
    }

    #[test]
    fn a_command_line_escapes_what_the_shell_would_read() {
        // A `=` at the start of a word is the `=cmd` expansion in zsh; an argument with spaces
        // must stay a single argument; an empty argument must not vanish; the remaining
        // metacharacters follow the drop's rule.
        assert_eq!(line(&["ssh", "=x"]), "ssh \\=x");
        assert_eq!(
            line(&["mosh", "--ssh=ssh -p 2", "prod"]),
            "mosh --ssh=ssh\\ -p\\ 2 prod"
        );
        assert_eq!(line(&["ssh", "-o", "", "prod"]), "ssh -o '' prod");
        assert_eq!(
            line(&["ssh", "-t", "prod", "a;b", "$HOME"]),
            "ssh -t prod a\\;b \\$HOME"
        );
        assert_eq!(line(&[]), "");
    }

    #[test]
    fn a_control_character_puts_its_argument_in_ansi_c_quotes() {
        // `\` + newline would be a line continuation (found in code review): the argument goes into
        // `$'…'`, and the `\` and `'` inside it are escaped.
        assert_eq!(
            line(&["ssh", "-t", "prod", "echo a\necho 'b'\\"]),
            "ssh -t prod $'echo a\\necho \\'b\\'\\\\'"
        );
        assert_eq!(
            line(&["ssh", "-t", "prod", "a\u{1b}b\tc"]),
            "ssh -t prod $'a\\u001bb\\tc'"
        );
    }

    #[test]
    fn paste_quote_escapes_a_single_line_like_a_drop() {
        // Without a newline the rule is exactly the drop's: same appearance.
        assert_eq!(
            paste_quote("/tmp/İki Kelime/a'b"),
            "/tmp/İki\\ Kelime/a\\'b"
        );
        assert_eq!(
            paste_quote("/tmp/İki Kelime/a'b"),
            quote("/tmp/İki Kelime/a'b")
        );
        assert_eq!(paste_quote(""), "");
    }

    #[test]
    fn paste_quote_wraps_multiline_text_in_single_quotes() {
        // Text with a newline goes entirely into single quotes: `\` + newline would be a line
        // continuation and the parts would be joined. An inner `'` is close-escape-reopen.
        assert_eq!(paste_quote("a b\nc"), "'a b\nc'");
        assert_eq!(paste_quote("it's\nok"), "'it'\\''s\nok'");
        // A Windows newline and a lone `\r` are reduced to a single `\n`: zsh's bracketed
        // reader already turns `\r` into `\n`, so `\r\n` would become two lines.
        assert_eq!(paste_quote("x\r\ny"), "'x\ny'");
        assert_eq!(paste_quote("x\ry"), "'x\ny'");
        // Inside the quote no metacharacter is escaped — literally.
        assert_eq!(paste_quote("$HOME\n`x`"), "'$HOME\n`x`'");
    }
}
