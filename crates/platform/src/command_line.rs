//! A Windows command line split as the C runtime splits it for a program:
//! the program's name first, then its arguments.

/// The arguments of `line`, as the program started with it sees them.
///
/// The program's name ends at the first space outside quotes; its quotes
/// only group, and backslashes mean nothing in it. In the rest, quotes
/// group words, `2n` backslashes before a quote are `n` backslashes,
/// `2n + 1` are `n` and a literal quote, and `""` inside quotes is a literal
/// quote too; backslashes elsewhere are themselves.
pub(crate) fn arguments(line: &str) -> Vec<String> {
    let line = line.trim_start_matches([' ', '\t']);
    if line.is_empty() {
        return Vec::new();
    }
    let mut chars = line.chars().peekable();
    let mut program = String::new();
    let mut quoted = false;
    while let Some(c) = chars.next_if(|&c| quoted || (c != ' ' && c != '\t')) {
        if c == '"' {
            quoted = !quoted;
        } else {
            program.push(c);
        }
    }
    let mut arguments = vec![program];
    loop {
        while chars.next_if(|&c| c == ' ' || c == '\t').is_some() {}
        if chars.peek().is_none() {
            return arguments;
        }
        let mut argument = String::new();
        let mut quoted = false;
        loop {
            let mut backslashes = 0;
            while chars.next_if_eq(&'\\').is_some() {
                backslashes += 1;
            }
            if chars.next_if_eq(&'"').is_some() {
                argument.extend(std::iter::repeat_n('\\', backslashes / 2));
                // Escaped by an odd backslash, or doubled inside quotes, the quote is text.
                if !backslashes.is_multiple_of(2) || (quoted && chars.next_if_eq(&'"').is_some()) {
                    argument.push('"');
                } else {
                    quoted = !quoted;
                }
                continue;
            }
            argument.extend(std::iter::repeat_n('\\', backslashes));
            match chars.peek() {
                None => break,
                Some(' ' | '\t') if !quoted => break,
                Some(&c) => {
                    argument.push(c);
                    chars.next();
                }
            }
        }
        arguments.push(argument);
    }
}

#[cfg(test)]
mod tests {
    use super::arguments;

    fn split(line: &str) -> Vec<String> {
        arguments(line)
    }

    #[test]
    fn takes_the_program_up_to_its_closing_quote_or_first_space() {
        assert_eq!(
            split(r#""C:\Program Files\nodejs\node.exe" server.js --port 3000"#),
            [
                "C:\\Program Files\\nodejs\\node.exe",
                "server.js",
                "--port",
                "3000"
            ]
        );
        assert_eq!(
            split(r"C:\Program Files\x.exe a"),
            ["C:\\Program", "Files\\x.exe", "a"]
        );
        assert_eq!(
            split(r#""C:\Program Files"\node.exe a.js"#),
            ["C:\\Program Files\\node.exe", "a.js"]
        );
        assert_eq!(split(r#"C:\x\"a b".exe c"#), ["C:\\x\\a b.exe", "c"]);
        assert_eq!(split(r#""C:\a\b.exe""#), ["C:\\a\\b.exe"]);
        assert_eq!(split(r#""unterminated"#), ["unterminated"]);
        assert_eq!(split("  node"), ["node"]);
    }

    #[test]
    fn splits_the_arguments_as_the_c_runtime_does() {
        let of = |rest: &str| split(&format!("x {rest}"))[1..].to_vec();
        assert_eq!(of(r#""a b c" d e"#), ["a b c", "d", "e"]);
        assert_eq!(of(r#""ab\"c" "\\" d"#), ["ab\"c", "\\", "d"]);
        assert_eq!(of(r#"a\\\b d"e f"g h"#), ["a\\\\\\b", "de fg", "h"]);
        assert_eq!(of(r#"a\\\"b c d"#), ["a\\\"b", "c", "d"]);
        assert_eq!(of(r#"a\\\\"b c" d e"#), ["a\\\\b c", "d", "e"]);
        assert_eq!(of(r#"a"b"" c d"#), ["ab\" c d"]);
    }

    #[test]
    fn keeps_an_empty_quoted_argument_and_a_shells_command_string() {
        assert_eq!(split(r#"x "" y"#), ["x", "", "y"]);
        assert_eq!(
            split(r#"C:\Windows\system32\cmd.exe /d /s /c "set X=1 && node a.js""#),
            [
                "C:\\Windows\\system32\\cmd.exe",
                "/d",
                "/s",
                "/c",
                "set X=1 && node a.js"
            ]
        );
        assert_eq!(split("x a\t b  "), ["x", "a", "b"]);
    }

    #[test]
    fn has_nothing_for_an_empty_line() {
        assert!(split("").is_empty());
        assert!(split(" \t ").is_empty());
    }
}
