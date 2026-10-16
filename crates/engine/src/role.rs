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
    // Some tools install their binary under a version number and are started
    // by a symlink, so argv[0] is the only place their name shows. Trusting it
    // for these roles only ever makes us stop less.
    let launched_as = args
        .and_then(|args| args.first())
        .map(|first| normalize(first));
    let is = |list: &[&str]| {
        list.contains(&name)
            || launched_as
                .as_deref()
                .is_some_and(|launched| list.contains(&launched))
    };
    let launches_an_agent = args.is_some_and(|args| {
        args.iter()
            .take(3)
            .any(|arg| AGENT_PACKAGES.iter().any(|package| arg.contains(package)))
    });
    if is(AGENTS) || launches_an_agent {
        return Role::Agent;
    }
    if is(TERMINALS) {
        return Role::Terminal;
    }
    if is(SHELLS) {
        return Role::Shell;
    }
    if RUNNERS.contains(&name) {
        return Role::Runner;
    }
    let first_arg = args.and_then(|args| args.get(1)).map(String::as_str);
    match name {
        "node" if first_arg.is_some_and(is_package_manager_script) => Role::Runner,
        "node" if is_titled_package_manager(args) => Role::Runner,
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

/// npm sets its process title ("npm run dev"), which replaces argv on macOS.
fn is_titled_package_manager(args: Option<&[String]>) -> bool {
    args.and_then(|args| args.first())
        .and_then(|first| first.split_whitespace().next())
        .is_some_and(|title| PACKAGE_MANAGERS.contains(&normalize(title).as_str()))
}

/// A runner only its process title vouches for. Titles are easy to fake, so
/// a climb may stop at such a runner but never pass it.
pub(crate) fn is_runner_by_title(name: &str, args: Option<&[String]>) -> bool {
    normalize(name) == "node"
        && is_titled_package_manager(args)
        && !args
            .and_then(|args| args.get(1))
            .is_some_and(|script| is_package_manager_script(script))
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
                    "--rcfile" | "--init-file" => {
                        rest.next();
                    }
                    "--login" | "--noprofile" | "--norc" | "--posix" | "--noediting"
                    | "--restricted" | "--verbose" => {}
                    // An option we can't read might take a value: call it a session.
                    _ if arg.starts_with("--") => return false,
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
    let script_at = script_position(rest);
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

/// Runtime options that take the next argument as their value.
const VALUE_OPTIONS: &[&str] = &[
    "-r",
    "--require",
    "--import",
    "--loader",
    "--experimental-loader",
    "-C",
    "--conditions",
    "--env-file",
    "--watch-path",
    "--title",
    "--inspect-port",
    "-X",
    "-W",
];

/// Where the script starts in a runtime's arguments: after the runtime's own
/// options, the values some of them take, and a `--` that ends them.
fn script_position(rest: &[String]) -> Option<usize> {
    let mut at = 0;
    while let Some(arg) = rest.get(at) {
        match arg.as_str() {
            "--" => return (at + 1 < rest.len()).then_some(at + 1),
            // Code on the command line rather than a script file.
            "-e" | "--eval" | "-p" | "--print" | "-c" => return None,
            option if VALUE_OPTIONS.contains(&option) => at += 2,
            option if option.starts_with('-') => at += 1,
            _ => return Some(at),
        }
    }
    None
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
    fn recognises_an_agent_whose_binary_is_named_after_its_version() {
        // ~/.local/share/claude/versions/2.1.300, started as `claude`.
        let claude = args(&["claude", "--resume"]);

        assert_eq!(role("2.1.300", Some(&claude)), Role::Agent);
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
    fn reads_the_values_of_bash_long_options() {
        // `-c` names the rcfile here; the shell is interactive.
        assert!(!runs_a_script(Some(&args(&[
            "bash", "--rcfile", "-c", "-i"
        ]))));
        assert!(runs_a_script(Some(&args(&[
            "bash",
            "--init-file",
            "x",
            "-c",
            "npm run dev"
        ]))));
        assert!(runs_a_script(Some(&args(&["bash", "--norc", "-c", "x"]))));
        assert!(!runs_a_script(Some(&args(&[
            "bash",
            "--mystery",
            "-c",
            "x"
        ]))));
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
    fn reads_node_options_the_way_node_does() {
        assert!(!supervises("node", &["node", "--", "--watch"]));
        assert!(supervises(
            "node",
            &["node", "-r", "ts-node/register", "--watch", "server.ts"]
        ));
        assert!(supervises(
            "node",
            &[
                "node",
                "--require",
                "dotenv/config",
                "/app/node_modules/.bin/next",
                "dev"
            ]
        ));
        assert!(!supervises(
            "python3",
            &["python3", "-X", "dev", "manage.py", "check"]
        ));
        assert!(supervises(
            "python3",
            &["python3", "-X", "dev", "manage.py", "runserver"]
        ));
    }

    #[test]
    fn code_given_on_the_command_line_is_not_a_script() {
        assert!(!supervises(
            "node",
            &[
                "node",
                "-e",
                "setInterval(() => {}, 1000) // /app/node_modules/next/x"
            ]
        ));
        assert!(!supervises(
            "node",
            &["node", "--print", "'/app/node_modules/next/x'"]
        ));
        assert!(!supervises(
            "python3",
            &["python3", "-c", "import manage", "runserver"]
        ));
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
