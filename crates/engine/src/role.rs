/// What a process is to us when we walk from a server up to whoever started it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    /// Hosts server code directly: node, python, ruby, java...
    Runtime,
    /// Starts something else that does the serving: npm run, cargo run, go run...
    Runner,
    Shell,
    Terminal,
    Agent,
    Other,
}

const RUNTIMES: &[&str] = &[
    "node",
    "bun",
    "deno",
    "python",
    "pythonw",
    "ruby",
    "php",
    "php-cgi",
    "java",
    "dotnet",
    "beam.smp",
    "erl",
    "elixir",
    "caddy",
    "uvicorn",
    "gunicorn",
    "hypercorn",
    "puma",
];
const RUNNERS: &[&str] = &[
    "npm", "npx", "pnpm", "yarn", "cargo", "go", "air", "mvn", "gradle", "gradlew", "uv", "poetry",
    "pipenv", "rails", "mix", "turbo", "nx",
];
const SHELLS: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "fish",
    "dash",
    "ksh",
    "csh",
    "tcsh",
    "nu",
    "cmd",
    "powershell",
    "pwsh",
];
const TERMINALS: &[&str] = &[
    "windowsterminal",
    "openconsole",
    "conhost",
    "terminal",
    "iterm2",
    "ghostty",
    "wezterm-gui",
    "kitty",
    "alacritty",
    "warp",
    "hyper",
    "tabby",
    "tmux",
    "screen",
    "code helper",
    "code helper (plugin)",
    "cursor helper",
    "cursor helper (plugin)",
];
const AGENTS: &[&str] = &[
    "claude",
    "codex",
    "gemini",
    "opencode",
    "aider",
    "cursor-agent",
];
const AGENT_PACKAGES: &[&str] = &[
    "@anthropic-ai/claude-code",
    "@openai/codex",
    "@google/gemini-cli",
    "opencode-ai",
];
const PACKAGE_MANAGERS: &[&str] = &["npm", "npx", "pnpm", "yarn"];

pub(crate) fn role(name: &str, args: Option<&[String]>) -> Role {
    let name = normalize(name);
    let name = name.as_str();
    let launches_an_agent = args.is_some_and(|args| {
        args.iter()
            .take(3)
            .any(|arg| AGENT_PACKAGES.iter().any(|package| arg.contains(package)))
    });
    if AGENTS.contains(&name) || launches_an_agent {
        return Role::Agent;
    }
    if TERMINALS.contains(&name) {
        return Role::Terminal;
    }
    if SHELLS.contains(&name) {
        return Role::Shell;
    }
    if RUNNERS.contains(&name) {
        return Role::Runner;
    }
    let first_arg = args.and_then(|args| args.get(1)).map(String::as_str);
    // A process title (npm sets "npm run dev") replaces argv on macOS.
    let title = args
        .and_then(|args| args.first())
        .and_then(|first| first.split_whitespace().next())
        .map(normalize);
    let titled_as_package_manager =
        title.is_some_and(|title| PACKAGE_MANAGERS.contains(&title.as_str()));
    match name {
        "node" if titled_as_package_manager || first_arg.is_some_and(is_package_manager_script) => {
            Role::Runner
        }
        "bun" | "deno" if matches!(first_arg, Some("run" | "task" | "x")) => Role::Runner,
        "dotnet"
            if args.is_some_and(|args| {
                args.iter()
                    .skip(1)
                    .any(|arg| arg == "run" || arg == "watch")
            }) =>
        {
            Role::Runner
        }
        _ if RUNTIMES.contains(&name) => Role::Runtime,
        _ => Role::Other,
    }
}

fn is_package_manager_script(script: &str) -> bool {
    let script = script.replace('\\', "/").to_lowercase();
    PACKAGE_MANAGERS
        .iter()
        .any(|manager| script.contains(&format!("/node_modules/{manager}/")))
}

/// `sh -c "next dev"` or `cmd /d /s /c "next dev"`: a shell given one command
/// string to run, as package managers do. A shell running a script file or
/// sitting at a prompt is somebody's session.
pub(crate) fn runs_a_script(args: Option<&[String]>) -> bool {
    let Some((program, rest)) = args.and_then(|args| args.split_first()) else {
        return false;
    };
    let mut lowered = rest.iter().map(|arg| arg.to_lowercase());
    match normalize(program.trim_start_matches('-')).as_str() {
        "cmd" => lowered
            .find_map(|arg| match arg.as_str() {
                "/c" | "/r" => Some(true),
                "/k" => Some(false),
                _ => None,
            })
            .unwrap_or(false),
        // Everything after -Command is the command, -NoExit included.
        "powershell" | "pwsh" => lowered
            .find_map(|arg| match arg.strip_prefix(['-', '/'])? {
                "command" | "c" | "encodedcommand" | "e" | "ec" => Some(true),
                "noexit" | "noe" | "file" | "f" => Some(false),
                _ => None,
            })
            .unwrap_or(false),
        _ => {
            let mut rest = rest.iter();
            while let Some(arg) = rest.next() {
                match arg.as_str() {
                    // `-` and `--` end the options; what follows is a script file.
                    "-" | "--" => return false,
                    _ if arg.starts_with("--") => {}
                    _ => match arg.strip_prefix(['-', '+']) {
                        Some(flags) if arg.starts_with('-') && flags.contains('c') => return true,
                        Some(flags) => {
                            // -o and -O take an option name: `bash -O extglob -c …`.
                            if flags.contains(['o', 'O']) {
                                rest.next();
                            }
                        }
                        // A positional before -c is a script file.
                        None => return false,
                    },
                }
            }
            false
        }
    }
}

/// Dev servers that run their real server in a child process and restart it,
/// so stopping only the child would bring it straight back.
pub(crate) fn is_dev_supervisor(name: &str, args: Option<&[String]>) -> bool {
    if role(name, args) != Role::Runtime {
        return false;
    }
    let Some((_, rest)) = args.and_then(|args| args.split_first()) else {
        return false;
    };
    let runtime = normalize(name);
    // The runtime's own options come before the script; the rest is the script's.
    let script_at = rest.iter().position(|arg| !arg.starts_with('-'));
    let options = &rest[..script_at.unwrap_or(rest.len())];
    let (tool, args) = match (runtime.as_str(), script_at) {
        ("node", _)
            if options
                .iter()
                .any(|option| option == "--watch" || option.starts_with("--watch-path")) =>
        {
            return true;
        }
        ("node" | "bun" | "deno", Some(at)) => {
            (crate::command::package_name(&rest[at]), &rest[at + 1..])
        }
        ("python", Some(at)) => (Some(normalize(&rest[at])), &rest[at + 1..]),
        _ => (Some(runtime), rest),
    };
    let subcommand = args.first().map(String::as_str);
    let has = |flag: &str| args.iter().any(|arg| arg == flag);
    match tool.as_deref() {
        Some(
            "next" | "nuxt" | "nuxi" | "astro" | "nodemon" | "ts-node-dev" | "react-scripts"
            | "storybook" | "@storybook/cli",
        ) => true,
        Some("tsx") => subcommand == Some("watch"),
        Some("nest" | "@nestjs/cli") => {
            subcommand == Some("start") && (has("--watch") || has("-w"))
        }
        Some("uvicorn" | "hypercorn") => has("--reload"),
        Some("manage.py") => subcommand == Some("runserver") && !has("--noreload"),
        _ => false,
    }
}

/// Lowercase, without `.exe`, and with Python's version suffix folded away.
pub(crate) fn normalize(name: &str) -> String {
    let base = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .to_lowercase();
    let stem = base.strip_suffix(".exe").unwrap_or(&base);
    match stem.strip_prefix("python") {
        Some(version) if version.chars().all(|c| c.is_ascii_digit() || c == '.') => "python".into(),
        _ => stem.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{Role, is_dev_supervisor, normalize, role, runs_a_script};

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn normalizes_platform_specific_names() {
        assert_eq!(normalize("node.exe"), "node");
        assert_eq!(normalize("Python"), "python");
        assert_eq!(normalize("python3.12"), "python");
        assert_eq!(normalize("WindowsTerminal.exe"), "windowsterminal");
    }

    #[test]
    fn node_running_a_package_manager_is_a_runner() {
        let npm = args(&[
            "node",
            "/usr/local/lib/node_modules/npm/bin/npm-cli.js",
            "run",
            "dev",
        ]);
        let pnpm = args(&[
            "node.exe",
            "C:\\Users\\a\\AppData\\Roaming\\npm\\node_modules\\pnpm\\bin\\pnpm.cjs",
            "dev",
        ]);

        assert_eq!(role("node", Some(&npm)), Role::Runner);
        assert_eq!(role("node.exe", Some(&pnpm)), Role::Runner);
    }

    #[test]
    fn recognises_a_package_manager_that_renamed_its_process() {
        // npm sets process.title, which overwrites argv on macOS.
        let titled = args(&["npm run dev   ", "", ""]);

        assert_eq!(role("node", Some(&titled)), Role::Runner);
    }

    #[test]
    fn node_running_a_server_is_a_runtime() {
        let vite = args(&["node", "/app/node_modules/.bin/vite", "--port", "5173"]);

        assert_eq!(role("node", Some(&vite)), Role::Runtime);
        assert_eq!(role("node", None), Role::Runtime);
    }

    #[test]
    fn build_tools_that_launch_the_server_are_runners() {
        assert_eq!(role("cargo", Some(&args(&["cargo", "run"]))), Role::Runner);
        assert_eq!(role("go", Some(&args(&["go", "run", "."]))), Role::Runner);
        assert_eq!(
            role("dotnet", Some(&args(&["dotnet", "watch", "run"]))),
            Role::Runner
        );
        assert_eq!(
            role("dotnet", Some(&args(&["dotnet", "Api.dll"]))),
            Role::Runtime
        );
        assert_eq!(
            role("bun", Some(&args(&["bun", "run", "dev"]))),
            Role::Runner
        );
        assert_eq!(
            role("bun", Some(&args(&["bun", "--hot", "server.ts"]))),
            Role::Runtime
        );
    }

    #[test]
    fn recognises_shells_terminals_and_agents() {
        assert_eq!(role("zsh", None), Role::Shell);
        assert_eq!(role("pwsh.exe", None), Role::Shell);
        assert_eq!(role("OpenConsole.exe", None), Role::Terminal);
        assert_eq!(role("iTerm2", None), Role::Terminal);
        assert_eq!(role("claude", None), Role::Agent);
        let claude_via_node = args(&[
            "node",
            "/opt/homebrew/lib/node_modules/@anthropic-ai/claude-code/cli.js",
        ]);
        assert_eq!(role("node", Some(&claude_via_node)), Role::Agent);
    }

    #[test]
    fn only_counts_explicit_command_strings_as_scripts() {
        assert!(!runs_a_script(Some(&args(&["cmd", "/k", "echo hello"]))));
        assert!(!runs_a_script(Some(&args(&["sh", "script.sh"]))));
        assert!(runs_a_script(Some(&args(&["bash", "-lc", "npm run dev"]))));
        assert!(runs_a_script(Some(&args(&[
            "pwsh.exe",
            "-NoLogo",
            "-Command",
            "npm run dev"
        ]))));
    }

    #[test]
    fn stops_reading_shell_options_where_the_shell_does() {
        assert!(!runs_a_script(Some(&args(&["bash", "--", "-c", "x"]))));
        assert!(!runs_a_script(Some(&args(&["bash", "-", "-c", "x"]))));
        assert!(runs_a_script(Some(&args(&[
            "bash",
            "-O",
            "extglob",
            "-c",
            "npm run dev"
        ]))));
        assert!(runs_a_script(Some(&args(&[
            "zsh", "+o", "nomatch", "-c", "x"
        ]))));
        assert!(runs_a_script(Some(&args(&[
            "bash", "-eo", "pipefail", "-c", "x"
        ]))));
        // -C is noclobber, not -c.
        assert!(!runs_a_script(Some(&args(&["bash", "-C", "start.sh"]))));
    }

    #[test]
    fn a_powershell_told_to_stay_open_is_a_session() {
        assert!(!runs_a_script(Some(&args(&[
            "pwsh", "-NoExit", "-Command", "x"
        ]))));
        assert!(!runs_a_script(Some(&args(&["pwsh", "-noe", "-c", "x"]))));
        assert!(runs_a_script(Some(&args(&[
            "pwsh", "-Command", "x", "-NoExit"
        ]))));
        assert!(!runs_a_script(Some(&args(&["pwsh", "-File", "start.ps1"]))));
    }

    #[test]
    fn cmd_treats_r_like_c() {
        assert!(runs_a_script(Some(&args(&["cmd", "/R", "next dev"]))));
    }

    fn supervises(name: &str, list: &[&str]) -> bool {
        is_dev_supervisor(name, Some(&args(list)))
    }

    #[test]
    fn knows_dev_servers_that_restart_a_child() {
        assert!(supervises(
            "node",
            &["node", "/app/node_modules/next/dist/bin/next", "dev"]
        ));
        assert!(supervises(
            "node",
            &["node", "/app/node_modules/.bin/nodemon", "server.js"]
        ));
        assert!(supervises(
            "node",
            &[
                "node",
                "/app/node_modules/.bin/tsx",
                "watch",
                "src/index.ts"
            ]
        ));
        assert!(supervises(
            "node",
            &["node", "/app/node_modules/.bin/nest", "start", "--watch"]
        ));
        assert!(supervises(
            "python3.12",
            &["python3", "-m", "uvicorn", "app:app", "--reload"]
        ));
        assert!(supervises(
            "Python",
            &["/usr/bin/python3", "manage.py", "runserver", "8000"]
        ));
    }

    #[test]
    fn reads_node_watch_mode_only_as_a_node_option() {
        assert!(supervises("node", &["node", "--watch", "server.js"]));
        assert!(supervises(
            "node",
            &["node", "--watch-path=src", "server.js"]
        ));
        assert!(!supervises("node", &["node", "server.js", "--watch"]));
    }

    #[test]
    fn a_stray_word_in_the_arguments_does_not_make_a_supervisor() {
        assert!(!supervises(
            "node",
            &["node", "/srv/supervisor.js", "--label", "watch"]
        ));
        assert!(!supervises(
            "node",
            &["node", "/app/node_modules/.bin/tsx", "src/watch.ts"]
        ));
        assert!(!supervises(
            "node",
            &["node", "/app/node_modules/.bin/nest", "start"]
        ));
        assert!(!supervises(
            "python3",
            &["python3", "-m", "uvicorn", "app:app"]
        ));
        assert!(!supervises(
            "python3",
            &["python3", "manage.py", "runserver", "--noreload"]
        ));
        assert!(!supervises(
            "python3",
            &["python3", "tools.py", "runserver"]
        ));
    }

    #[test]
    fn tells_a_script_shell_from_an_interactive_one() {
        assert!(runs_a_script(Some(&args(&["sh", "-c", "next dev"]))));
        assert!(runs_a_script(Some(&args(&[
            "C:\\Windows\\system32\\cmd.exe",
            "/d",
            "/s",
            "/c",
            "next dev"
        ]))));
        assert!(!runs_a_script(Some(&args(&["-zsh"]))));
        assert!(!runs_a_script(None));
    }
}
