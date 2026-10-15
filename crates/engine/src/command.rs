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
    let Some((program, rest)) = args.split_first() else {
        return String::new();
    };
    let runtime = normalize(program);
    let mut words: Vec<String> = Vec::with_capacity(args.len());
    match runtime.as_str() {
        "node" | "bun" | "deno" if rest.first().is_some_and(|arg| is_path(arg)) => {
            match package_name(&rest[0]) {
                Some(package) => words.push(package),
                None => words.extend([runtime.clone(), file_name(&rest[0])]),
            }
            words.extend(rest[1..].iter().cloned());
        }
        "python" if rest.len() >= 2 && rest[0] == "-m" => words.extend(rest[1..].iter().cloned()),
        _ => {
            words.push(file_name(program));
            words.extend(rest.iter().cloned());
        }
    }
    // Split words a process title padded or a shell received as one string,
    // so secrets inside `sh -c "server --token=…"` are masked too.
    let tokens: Vec<String> = words
        .iter()
        .flat_map(|word| word.split_whitespace())
        .map(|token| {
            if is_path(token) {
                file_name(token)
            } else {
                token.to_string()
            }
        })
        .collect();
    mask(tokens).join(" ")
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

fn mask(words: Vec<String>) -> Vec<String> {
    let mut masked = Vec::with_capacity(words.len());
    let mut hide_next = false;
    for word in words {
        if hide_next {
            masked.push("***".to_string());
            hide_next = false;
        } else if let Some((flag, _)) = word.split_once('=')
            && is_secret_flag(flag)
        {
            masked.push(format!("{flag}=***"));
        } else if let Some((name, _)) = word.split_once('=')
            && is_secret_name(name)
        {
            masked.push(format!("{name}=***"));
        } else if is_secret_flag(&word) || word.eq_ignore_ascii_case("bearer") {
            masked.push(word);
            hide_next = true;
        } else {
            masked.push(mask_url(&word));
        }
    }
    masked
}

fn is_secret_flag(word: &str) -> bool {
    word.starts_with('-') && mentions_a_credential(word.trim_start_matches('-'))
}

fn mentions_a_credential(name: &str) -> bool {
    let name = name.to_ascii_lowercase().replace('_', "-");
    CREDENTIAL_PARTS.iter().any(|part| name.contains(part))
}

/// `GITHUB_TOKEN`, `X_API_KEY`, `DB_PASSWORD`…
fn is_secret_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && mentions_a_credential(name)
}

/// Masks the password in the authority and any credential in the query string.
fn mask_url(word: &str) -> String {
    let word = mask_url_password(word);
    let Some((base, query)) = word.split_once('?') else {
        return word;
    };
    if !base.contains("://") {
        return word;
    }
    let query: Vec<String> = query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((key, _)) if mentions_a_credential(key) => format!("{key}=***"),
            _ => pair.to_string(),
        })
        .collect();
    format!("{base}?{}", query.join("&"))
}

/// `scheme://user:password@host` → `scheme://user:***@host`
fn mask_url_password(word: &str) -> String {
    let Some(scheme_end) = word.find("://") else {
        return word.to_string();
    };
    let authority_start = scheme_end + 3;
    let Some(at) = word[authority_start..]
        .find('@')
        .map(|at| authority_start + at)
    else {
        return word.to_string();
    };
    match word[authority_start..at].find(':') {
        Some(colon) => format!("{}***{}", &word[..authority_start + colon + 1], &word[at..]),
        None => word.to_string(),
    }
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
            show(&["env", "GITHUB_TOKEN=ghp_1", "PORT=3000"]),
            "env GITHUB_TOKEN=*** PORT=3000"
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
            "curl -H Authorization: Bearer *** http://localhost:3000"
        );
        assert_eq!(
            show(&["open", "http://localhost:3000/cb?token=abc&mode=dev"]),
            "open http://localhost:3000/cb?token=***&mode=dev"
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
