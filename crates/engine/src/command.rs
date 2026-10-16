use std::ops::Range;

use crate::role::normalize;

/// Parts of a flag or variable name that mark its value as a credential.
const CREDENTIAL_PARTS: &[&str] = &[
    "token",
    "secret",
    "password",
    "passwd",
    "api-key",
    "apikey",
    "access-key",
    "private-key",
    "credential",
    "auth",
    "cookie",
];

/// What someone would have typed to start the process, with secrets masked.
pub(crate) fn display(args: &[String]) -> String {
    // A process title leaves blank and empty arguments behind it. They stay
    // until masking is done, so `--password ""` keeps its empty value.
    let args: Vec<&str> = args.iter().map(|arg| arg.trim()).collect();
    let Some((&program, rest)) = args.split_first() else {
        return String::new();
    };
    let runtime = normalize(program);
    let mut words: Vec<String> = Vec::with_capacity(args.len());
    let rest = match runtime.as_str() {
        "node" | "bun" | "deno" if rest.first().is_some_and(|arg| is_path(arg)) => {
            match package_name(rest[0]) {
                Some(package) => words.push(package),
                None => words.extend([runtime.clone(), file_name(rest[0])]),
            }
            &rest[1..]
        }
        "python" if rest.len() >= 2 && rest[0] == "-m" => &rest[1..],
        _ => {
            words.push(file_name(program));
            rest
        }
    };
    words.extend(rest.iter().map(|arg| arg.to_string()));
    let masked: Vec<String> = mask(&words)
        .into_iter()
        .filter(|word| !word.is_empty())
        .collect();
    masked.join(" ")
}

/// `…/node_modules/.bin/vite` → `vite`, `…/node_modules/@storybook/cli/…` → `@storybook/cli`.
pub(crate) fn package_name(path: &str) -> Option<String> {
    let path = path.replace('\\', "/");
    let (_, inside) = path.rsplit_once("node_modules/")?;
    let mut segments = inside.split('/').filter(|segment| !segment.is_empty());
    match segments.next()? {
        ".bin" => segments.next().map(str::to_string),
        scope if scope.starts_with('@') => segments.next().map(|name| format!("{scope}/{name}")),
        name => Some(name.to_string()),
    }
}

fn is_path(word: &str) -> bool {
    let bytes = word.as_bytes();
    word.starts_with('/')
        || (bytes.len() > 2 && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/'))
}

fn file_name(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or(path)
        .to_string()
}

const HIDDEN: &str = "***";

/// What a masked word means for the words after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum After {
    Nothing,
    /// A secret's value may run on past the next space, as quoting allows.
    RestHidden,
    /// The word is a secret flag or header whose value comes next.
    NextHidden,
}

fn mask(words: &[String]) -> Vec<String> {
    let mut masked = Vec::with_capacity(words.len());
    let mut hide_next = false;
    for word in words {
        if hide_next {
            masked.push(HIDDEN.to_string());
            hide_next = false;
        } else if let Some(name) = whole_assignment(word) {
            masked.push(format!("{name}={HIDDEN}"));
        } else {
            let (text, after) = mask_argument(word);
            masked.push(text);
            hide_next = after == After::NextHidden;
        }
    }
    masked
}

/// `NAME` when the whole argument is `NAME=value`. An environment entry that
/// a rewritten argv let slip in looks just like that, and its name can be
/// anything, so the value goes whatever the name says.
fn whole_assignment(word: &str) -> Option<&str> {
    let (name, _) = word.split_once('=')?;
    let plain = !name.is_empty()
        && !name.starts_with('-')
        && !name.contains(char::is_whitespace)
        && !name.contains("://");
    plain.then_some(name)
}

/// An argument with spaces may be a shell's command string or a process
/// title, where quoting can carry a value past the next space, so everything
/// after a secret in it is hidden.
fn mask_argument(argument: &str) -> (String, After) {
    let mut words = Vec::new();
    let mut tokens = argument.split_whitespace().peekable();
    while let Some(token) = tokens.next() {
        let token = if is_path(token) {
            file_name(token)
        } else {
            token.to_string()
        };
        let (text, after) = mask_token(&token);
        words.push(text);
        match after {
            After::Nothing => {}
            After::RestHidden => break,
            After::NextHidden if tokens.peek().is_none() => {
                return (words.join(" "), After::NextHidden);
            }
            After::NextHidden => {
                words.push(HIDDEN.to_string());
                break;
            }
        }
    }
    (words.join(" "), After::Nothing)
}

/// Characters after which a shell starts a new word even without a space.
const SHELL_OPERATORS: &[char] = &['&', ';', '|', '(', ')', '<', '>'];

/// Every detector marks what to hide in the word, and all of it is hidden at
/// once, so no detector's output can shield a secret another one found.
fn mask_token(token: &str) -> (String, After) {
    // Quotes and escapes don't change what a shell hands on: `"--to\ken=x"` is `--token=x`.
    let normal: String = token
        .chars()
        .filter(|c| !matches!(c, '"' | '\'' | '`' | '\\' | '^'))
        .collect();
    let mut hidden: Vec<Range<usize>> = Vec::new();
    let mut after = After::Nothing;
    let mut offset = 0;
    // `&` stays inside URLs, where it separates query pairs.
    for segment in normal.split([';', '|', '(', ')', '<', '>']) {
        if segment.contains("://") {
            hide_url_secrets(segment, offset, &mut hidden);
        } else {
            let mut start = offset;
            for part in segment.split('&') {
                if hide_assignment_secrets(part, start, normal.len(), &mut hidden) {
                    after = After::RestHidden;
                }
                start += part.len() + 1;
            }
        }
        offset += segment.len() + 1;
    }
    let last = normal.rsplit(SHELL_OPERATORS).next().unwrap_or_default();
    let flag = !last.contains('=') && (is_secret_flag(last) || last.eq_ignore_ascii_case("bearer"));
    let header = last
        .strip_suffix(':')
        .is_some_and(|name| mentions_a_credential(name_at_end(name)));
    if after == After::Nothing && (flag || header) {
        after = After::NextHidden;
    }
    if after == After::Nothing && hidden.is_empty() {
        return (token.to_string(), after);
    }
    (with_hidden(&normal, hidden), after)
}

/// `--secret-flag=value`, `API_TOKEN=value`, `X-Api-Key:value` in one word.
/// Returns whether the rest of the argument has to go too. Any other
/// `name=value` keeps its name only: it may be the environment.
fn hide_assignment_secrets(
    part: &str,
    offset: usize,
    end: usize,
    hidden: &mut Vec<Range<usize>>,
) -> bool {
    for (at, separator) in part.match_indices(['=', ':']) {
        // `Authorization:` ending the word has its value in the next one.
        if separator == ":" && offset + at + 1 == end {
            break;
        }
        let name = &part[..at];
        if is_secret_flag(name) || mentions_a_credential(name_at_end(name)) {
            hidden.push(offset + at + 1..end);
            return true;
        }
    }
    if let Some(at) = part.find('=')
        && !part.starts_with('-')
    {
        hidden.push(offset + at + 1..offset + part.len());
    }
    false
}

/// A URL's user info, whole, and query values whose key names a credential.
fn hide_url_secrets(segment: &str, offset: usize, hidden: &mut Vec<Range<usize>>) {
    let Some(scheme_end) = segment.find("://") else {
        return;
    };
    let start = scheme_end + 3;
    let rest = &segment[start..];
    // Passwords often hold `@`, `/` or `#` unencoded, so with a colon in the
    // user info everything up to the last `@` goes. A user name alone may be
    // a token; it ends where the host does, so `/@types/node` stays.
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let user_end = match rest.rfind('@') {
        Some(at) if rest[..at].contains(':') => Some(at),
        _ => rest[..host_end].rfind('@'),
    };
    if let Some(at) = user_end {
        hidden.push(offset + start..offset + start + at);
    }
    let after_user = user_end.map_or(start, |at| start + at + 1);
    let Some(query) = segment[after_user..].find('?') else {
        return;
    };
    let query_start = after_user + query + 1;
    let query_end = segment[query_start..]
        .find('#')
        .map_or(segment.len(), |end| query_start + end);
    let mut pair_start = query_start;
    for pair in segment[query_start..query_end].split('&') {
        if let Some((key, _)) = pair.split_once('=')
            && is_credential_key(key)
        {
            hidden.push(offset + pair_start + key.len() + 1..offset + pair_start + pair.len());
        }
        pair_start += pair.len() + 1;
    }
}

/// Replaces the marked ranges, merged, with the mask.
fn with_hidden(text: &str, mut hidden: Vec<Range<usize>>) -> String {
    hidden.sort_by_key(|range| range.start);
    let mut shown = String::with_capacity(text.len());
    let mut cursor = 0;
    for range in hidden {
        if range.start < cursor {
            cursor = cursor.max(range.end);
            continue;
        }
        shown.push_str(&text[cursor..range.start]);
        shown.push_str(HIDDEN);
        cursor = range.end;
    }
    shown.push_str(&text[cursor.min(text.len())..]);
    shown
}

/// `API_TOKEN` in `true;API_TOKEN`, `X-Api-Key` in `-HX-Api-Key`.
fn name_at_end(text: &str) -> &str {
    let start = text
        .char_indices()
        .rev()
        .find(|&(_, c)| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .map_or(0, |(at, c)| at + c.len_utf8());
    &text[start..]
}

fn is_secret_flag(word: &str) -> bool {
    word.starts_with('-') && mentions_a_credential(word.trim_start_matches('-'))
}

fn mentions_a_credential(name: &str) -> bool {
    let name = name.to_ascii_lowercase().replace('_', "-");
    CREDENTIAL_PARTS.iter().any(|part| name.contains(part))
}

/// Query keys that carry a credential without saying so in their name.
const QUERY_CREDENTIALS: &[&str] = &["key", "sig", "signature", "jwt", "sid"];

/// Compared the way a server reads it: `%74oken` is `token`.
fn is_credential_key(key: &str) -> bool {
    let key = percent_decoded(key);
    mentions_a_credential(&key)
        || QUERY_CREDENTIALS
            .iter()
            .any(|known| key.eq_ignore_ascii_case(known))
}

fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let escape = bytes[at] == b'%'
            && bytes.get(at + 1).is_some_and(u8::is_ascii_hexdigit)
            && bytes.get(at + 2).is_some_and(u8::is_ascii_hexdigit);
        match escape
            .then(|| u8::from_str_radix(&text[at + 1..at + 3], 16).ok())
            .flatten()
        {
            Some(byte) => {
                decoded.push(byte);
                at += 3;
            }
            None => {
                decoded.push(bytes[at]);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

#[cfg(test)]
mod tests {
    use super::display;

    fn show(list: &[&str]) -> String {
        display(&list.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn shows_package_manager_scripts_as_typed() {
        assert_eq!(
            show(&[
                "node",
                "/usr/local/lib/node_modules/npm/bin/npm-cli.js",
                "run",
                "dev"
            ]),
            "npm run dev"
        );
        assert_eq!(
            show(&[
                "C:\\Program Files\\nodejs\\node.exe",
                "C:\\Users\\a\\AppData\\Roaming\\npm\\node_modules\\pnpm\\bin\\pnpm.cjs",
                "dev"
            ]),
            "pnpm dev"
        );
    }

    #[test]
    fn shows_local_binaries_by_package_name() {
        assert_eq!(
            show(&["node", "/app/node_modules/.bin/vite", "--port", "5173"]),
            "vite --port 5173"
        );
        assert_eq!(
            show(&["node", "/app/node_modules/next/dist/bin/next", "dev"]),
            "next dev"
        );
        assert_eq!(
            show(&[
                "node",
                "/app/node_modules/@storybook/cli/bin/index.cjs",
                "dev"
            ]),
            "@storybook/cli dev"
        );
    }

    #[test]
    fn shows_python_modules_by_module_name() {
        assert_eq!(
            show(&[
                "/usr/bin/python3",
                "-m",
                "uvicorn",
                "app.main:app",
                "--reload"
            ]),
            "uvicorn app.main:app --reload"
        );
    }

    #[test]
    fn shortens_absolute_paths_to_file_names() {
        assert_eq!(
            show(&["/usr/local/bin/node", "/home/a/api/server.js"]),
            "node server.js"
        );
        assert_eq!(
            show(&["C:\\dev\\api\\target\\debug\\api.exe", "--port", "8080"]),
            "api.exe --port 8080"
        );
    }

    #[test]
    fn masks_secrets_inside_a_shell_command_string() {
        assert_eq!(
            show(&["sh", "-c", "server --token=abc"]),
            "sh -c server --token=***"
        );
        assert_eq!(
            show(&["sh", "-c", "server --password hunter2"]),
            "sh -c server --password ***"
        );
    }

    #[test]
    fn masks_secret_looking_assignments() {
        assert_eq!(
            show(&["env", "X_API_KEY=abc", "node", "a.js"]),
            "env X_API_KEY=*** node a.js"
        );
        assert_eq!(
            show(&["env", "GITHUB_TOKEN=ghp_1", "PORT=3000", "node", "a.js"]),
            "env GITHUB_TOKEN=*** PORT=*** node a.js"
        );
    }

    #[test]
    fn drops_the_padding_a_process_title_leaves_behind() {
        assert_eq!(show(&["npm run dev   ", "", "  "]), "npm run dev");
    }

    #[test]
    fn masks_any_flag_whose_name_mentions_a_credential() {
        assert_eq!(
            show(&["deploy", "--github-token", "ghp_1"]),
            "deploy --github-token ***"
        );
        assert_eq!(
            show(&["api", "--db-password=pw", "--port", "80"]),
            "api --db-password=*** --port 80"
        );
        assert_eq!(
            show(&["api", "--client_secret", "s"]),
            "api --client_secret ***"
        );
    }

    #[test]
    fn masks_bearer_tokens_and_secret_query_parameters() {
        assert_eq!(
            show(&[
                "curl",
                "-H",
                "Authorization: Bearer abc.def",
                "http://localhost:3000"
            ]),
            "curl -H Authorization: *** http://localhost:3000"
        );
        assert_eq!(
            show(&["open", "http://localhost:3000/cb?token=abc&mode=dev"]),
            "open http://localhost:3000/cb?token=***&mode=dev"
        );
    }

    #[test]
    fn hides_a_whole_argument_after_a_secret_flag() {
        assert_eq!(
            show(&["server", "--password", "alpha beta", "--port", "80"]),
            "server --password *** --port 80"
        );
    }

    #[test]
    fn hides_the_rest_of_a_shell_string_after_a_secret() {
        assert_eq!(
            show(&["sh", "-c", "server --password 'alpha beta' --port 80"]),
            "sh -c server --password ***"
        );
        assert_eq!(
            show(&["sh", "-c", "server --token=\"a b\" --port 80"]),
            "sh -c server --token=***"
        );
    }

    #[test]
    fn sees_through_quotes_and_escapes_around_a_secret_flag() {
        assert_eq!(
            show(&["sh", "-c", "server \"--token=abc\""]),
            "sh -c server --token=***"
        );
        assert_eq!(
            show(&["sh", "-c", "server --pass\"word\" hunter2"]),
            "sh -c server --password ***"
        );
    }

    #[test]
    fn masks_powershell_environment_assignments() {
        assert_eq!(
            show(&["pwsh", "-Command", "$env:API_TOKEN='x'; npm run dev"]),
            "pwsh -Command $env:API_TOKEN=***"
        );
    }

    #[test]
    fn masks_secrets_in_a_process_title() {
        assert_eq!(
            show(&["server --password alpha beta", "", ""]),
            "server --password ***"
        );
    }

    #[test]
    fn hides_the_next_argument_when_a_shell_string_ends_with_a_secret_flag() {
        assert_eq!(
            show(&["sh", "-c", "server --password", "hunter2"]),
            "sh -c server --password ***"
        );
    }

    #[test]
    fn an_empty_secret_still_counts_as_the_flags_value() {
        assert_eq!(
            show(&["server", "--password", "", "--token", "sentinel"]),
            "server --password *** --token ***"
        );
    }

    #[test]
    fn finds_an_assignment_wherever_its_name_ends() {
        assert_eq!(
            show(&["sh", "-c", "true;API_TOKEN=sentinel server"]),
            "sh -c true;API_TOKEN=***"
        );
        assert_eq!(
            show(&["docker", "run", "--env=API_TOKEN=sentinel", "img"]),
            "docker run --env=API_TOKEN=*** img"
        );
    }

    #[test]
    fn masks_trailing_assignments_that_may_be_the_environment() {
        assert_eq!(show(&["aXb", "SESSION=sentinel"]), "aXb SESSION=***");
        assert_eq!(
            show(&["aXbXc", "SESSION=sentinel", "A.B=x"]),
            "aXbXc SESSION=*** A.B=***"
        );
    }

    #[test]
    fn splits_words_where_a_shell_operator_does() {
        assert_eq!(
            show(&["sh", "-c", "x;--password hunter2"]),
            "sh -c x;--password ***"
        );
        assert_eq!(
            show(&["sh", "-c", "build&&--token abc"]),
            "sh -c build&&--token ***"
        );
        assert_eq!(
            show(&["sh", "-c", "curl https://h/a?x=1;API_TOKEN=secret run"]),
            "sh -c curl https://h/a?x=1;API_TOKEN=***"
        );
    }

    #[test]
    fn survives_names_that_are_not_ascii() {
        assert_eq!(show(&["x", "é=1", "--ünïcode=2"]), "x é=*** --ünïcode=2");
        assert_eq!(show(&["x", "ключ_token=1", "y"]), "x ключ_token=*** y");
    }

    #[test]
    fn hides_a_token_used_as_the_user_name_in_a_url() {
        assert_eq!(
            show(&["git", "clone", "https://ghp_abc123@github.com/o/r.git"]),
            "git clone https://***@github.com/o/r.git"
        );
    }

    #[test]
    fn masks_passwords_that_hold_url_characters() {
        assert_eq!(
            show(&["psql", "postgres://u:p#ss@h/db"]),
            "psql postgres://***@h/db"
        );
        assert_eq!(
            show(&["psql", "postgres://u:p@ss/w@h/db"]),
            "psql postgres://***@h/db"
        );
    }

    #[test]
    fn masks_values_after_a_credential_name_and_a_colon() {
        assert_eq!(
            show(&["curl", "-H", "X-Api-Key: abc", "http://x"]),
            "curl -H X-Api-Key: *** http://x"
        );
        assert_eq!(
            show(&["app", "--config", "{\"password\":\"x\"}"]),
            "app --config {password:***"
        );
    }

    #[test]
    fn leaves_an_at_sign_in_the_path_alone() {
        assert_eq!(
            show(&["npm", "view", "https://registry.npmjs.org/@types/node"]),
            "npm view https://registry.npmjs.org/@types/node"
        );
    }

    #[test]
    fn hides_every_kind_of_authorization_and_cookie_header() {
        assert_eq!(
            show(&[
                "curl",
                "-H",
                "Authorization: Basic dXNlcjpwYXNz",
                "http://x"
            ]),
            "curl -H Authorization: *** http://x"
        );
        assert_eq!(
            show(&["curl", "-H", "Cookie: session=abc; theme=dark"]),
            "curl -H Cookie: ***"
        );
        assert_eq!(
            show(&["curl", "--header=Proxy-Authorization:Digest abc"]),
            "curl --header=Proxy-Authorization:***"
        );
    }

    #[test]
    fn masks_query_keys_that_carry_credentials() {
        assert_eq!(
            show(&["open", "https://maps.example.com/api?key=AIza1&q=x&sig=s1"]),
            "open https://maps.example.com/api?key=***&q=x&sig=***"
        );
    }

    #[test]
    fn one_secret_never_shields_another() {
        assert_eq!(
            show(&["open", "https://h/?token=sentinel&auth=x"]),
            "open https://h/?token=***&auth=***"
        );
        assert_eq!(
            show(&["git", "clone", "https://ghp_sentinel:x@h/r"]),
            "git clone https://***@h/r"
        );
    }

    #[test]
    fn reads_query_keys_the_way_a_server_would() {
        assert_eq!(
            show(&["open", "https://h/?%74oken=sentinel&q=1"]),
            "open https://h/?%74oken=***&q=1"
        );
    }

    #[test]
    fn masks_secrets_in_flags_and_urls() {
        assert_eq!(show(&["server", "--token=abc123"]), "server --token=***");
        assert_eq!(
            show(&["server", "--password", "hunter2", "--port", "80"]),
            "server --password *** --port 80"
        );
        assert_eq!(show(&["worker", "--api-key", "k"]), "worker --api-key ***");
        assert_eq!(
            show(&["worker", "postgres://app:s3cret@localhost:5432/app"]),
            "worker postgres://***@localhost:5432/app"
        );
    }
}
