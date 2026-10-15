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

fn mask(words: &[String]) -> Vec<String> {
    // The environment sits right after the arguments in memory, and a
    // process that rewrote its arguments can shift some of it into view. Only
    // the tail can be that, so any assignment there is masked.
    let tail = words
        .iter()
        .rposition(|word| !word.is_empty() && assignment_name(word).is_none())
        .map_or(0, |last| last + 1);
    let mut masked = Vec::with_capacity(words.len());
    let mut hide_next = false;
    for (index, word) in words.iter().enumerate() {
        if hide_next {
            masked.push(HIDDEN.to_string());
            hide_next = false;
        } else if let Some(name) = assignment_name(word).filter(|_| index >= tail) {
            masked.push(format!("{name}={HIDDEN}"));
        } else {
            let (text, value_follows) = mask_argument(word);
            masked.push(text);
            hide_next = value_follows;
        }
    }
    masked
}

/// `NAME` in `NAME=value`, for a shell-style variable name.
fn assignment_name(word: &str) -> Option<&str> {
    let (name, _) = word.split_once('=')?;
    let mut chars = name.chars();
    let first = chars.next()?;
    let valid = (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    valid.then_some(name)
}

/// An argument with spaces may be a shell's command string or a process
/// title, where quoting can carry a value past the next space, so everything
/// after a secret in it is hidden. Also says whether the argument ended on a
/// flag whose value is the next argument.
fn mask_argument(argument: &str) -> (String, bool) {
    let mut words = Vec::new();
    let mut tokens = argument.split_whitespace().peekable();
    while let Some(token) = tokens.next() {
        let token = if is_path(token) {
            file_name(token)
        } else {
            token.to_string()
        };
        match secret(&token) {
            Some(Secret::Kept(prefix)) => {
                words.push(format!("{prefix}{HIDDEN}"));
                break;
            }
            Some(Secret::Follows(flag)) => {
                words.push(flag);
                if tokens.peek().is_none() {
                    return (words.join(" "), true);
                }
                words.push(HIDDEN.to_string());
                break;
            }
            None => words.push(mask_url(&token)),
        }
    }
    (words.join(" "), false)
}

enum Secret {
    /// `--token=abc`, `Cookie:abc`: what comes before the value, separator included.
    Kept(String),
    /// `--token`, `Bearer`, `Authorization:`: the value comes next.
    Follows(String),
}

/// Headers whose whole value is a credential, whatever its scheme.
const SECRET_HEADERS: &[&str] = &["authorization:", "cookie:"];

fn secret(token: &str) -> Option<Secret> {
    // Quotes and escapes don't change what a shell hands on: `"--to\ken=x"` is `--token=x`.
    let token: String = token
        .chars()
        .filter(|c| !matches!(c, '"' | '\'' | '`' | '\\' | '^'))
        .collect();
    // A shell starts a new word after an operator even without a space:
    // `x;--password`, `build&&--token`.
    let mut offset = 0;
    for piece in token.split(SHELL_OPERATORS) {
        // A URL's query is mask_url's business.
        let head = match (piece.find("://"), piece.find('?')) {
            (Some(scheme), Some(query)) if scheme < query => &piece[..query],
            _ => piece,
        };
        // Any `=` can end a name: `--env=API_TOKEN=x`, `$env:API_TOKEN=x`.
        for (at, _) in head.match_indices('=') {
            let name = &head[..at];
            if is_secret_flag(name) || mentions_a_credential(identifier_at_end(name)) {
                return Some(Secret::Kept(token[..=offset + at].to_string()));
            }
        }
        offset += piece.len() + 1;
    }
    // Lowercasing ASCII keeps every byte where it was.
    let lowered = token.to_ascii_lowercase();
    if let Some(end) = SECRET_HEADERS
        .iter()
        .find_map(|header| lowered.find(header).map(|at| at + header.len()))
    {
        return Some(if end == token.len() {
            Secret::Follows(token)
        } else {
            Secret::Kept(token[..end].to_string())
        });
    }
    let last = token.rsplit(SHELL_OPERATORS).next().unwrap_or_default();
    let follows =
        !last.contains('=') && (is_secret_flag(last) || last.eq_ignore_ascii_case("bearer"));
    follows.then_some(Secret::Follows(token))
}

const SHELL_OPERATORS: &[char] = &[';', '|', '&', '(', ')', '<', '>'];

/// `API_TOKEN` in `true;API_TOKEN`.
fn identifier_at_end(text: &str) -> &str {
    let start = text
        .char_indices()
        .rev()
        .find(|&(_, c)| !(c.is_ascii_alphanumeric() || c == '_'))
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
const QUERY_CREDENTIALS: &[&str] = &["key", "sig", "signature", "auth", "jwt", "sid"];

/// Masks credentials in the authority and in the query string.
fn mask_url(word: &str) -> String {
    let word = mask_url_user(word);
    let Some((base, query)) = word.split_once('?') else {
        return word;
    };
    if !base.contains("://") {
        return word;
    }
    let query: Vec<String> = query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((key, _))
                if mentions_a_credential(key)
                    || QUERY_CREDENTIALS
                        .iter()
                        .any(|known| key.eq_ignore_ascii_case(known)) =>
            {
                format!("{key}={HIDDEN}")
            }
            _ => pair.to_string(),
        })
        .collect();
    format!("{base}?{}", query.join("&"))
}

/// `scheme://user:password@host` → `scheme://user:***@host`. A user name
/// alone may be a token (`https://ghp_…@github.com`), so it goes entirely.
fn mask_url_user(word: &str) -> String {
    let Some(scheme_end) = word.find("://") else {
        return word.to_string();
    };
    let start = scheme_end + 3;
    let authority = word[start..]
        .find(['/', '?', '#'])
        .map_or(&word[start..], |end| &word[start..start + end]);
    let Some(at) = authority.rfind('@') else {
        return word.to_string();
    };
    let kept = authority[..at]
        .find(':')
        .map_or(start, |colon| start + colon + 1);
    format!("{}{HIDDEN}{}", &word[..kept], &word[start + at..])
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
            "env GITHUB_TOKEN=*** PORT=3000 node a.js"
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
            show(&["env", "PORT=3000", "node", "a.js"]),
            "env PORT=3000 node a.js"
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
        assert_eq!(show(&["x", "é=1", "--ünïcode=2"]), "x é=1 --ünïcode=2");
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
    fn masks_secrets_in_flags_and_urls() {
        assert_eq!(show(&["server", "--token=abc123"]), "server --token=***");
        assert_eq!(
            show(&["server", "--password", "hunter2", "--port", "80"]),
            "server --password *** --port 80"
        );
        assert_eq!(show(&["worker", "--api-key", "k"]), "worker --api-key ***");
        assert_eq!(
            show(&["worker", "postgres://app:s3cret@localhost:5432/app"]),
            "worker postgres://app:***@localhost:5432/app"
        );
    }
}
