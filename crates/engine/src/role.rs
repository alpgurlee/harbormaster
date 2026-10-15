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
    match name {
        "node" if first_arg.is_some_and(is_package_manager_script) => Role::Runner,
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
    let lowered = |arg: &String| arg.to_lowercase();
    match normalize(program.trim_start_matches('-')).as_str() {
        "cmd" => {
            for arg in rest.iter().map(lowered) {
                match arg.as_str() {
                    "/k" => return false,
                    "/c" => return true,
                    _ => {}
                }
            }
            false
        }
        "powershell" | "pwsh" => rest.iter().map(lowered).any(|arg| {
            matches!(
                arg.as_str(),
                "-command" | "-c" | "-file" | "-encodedcommand"
            )
        }),
        _ => {
            for arg in rest {
                if arg.starts_with("--") {
                    continue;
                }
                match arg.strip_prefix('-') {
                    Some(flags) if flags.contains('c') => return true,
                    Some(_) => continue,
                    // A positional before -c is a script file.
                    None => return false,
                }
            }
            false
        }
    }
}

/// Dev servers that run their real server in a child process and restart it,
/// so stopping only the child would bring it straight back.
const DEV_SUPERVISORS: &[&str] = &[
    "next",
    "nuxt",
    "astro",
    "nodemon",
    "ts-node-dev",
    "react-scripts",
    "storybook",
    "@storybook/cli",
];

pub(crate) fn is_dev_supervisor(name: &str, args: Option<&[String]>) -> bool {
    if role(name, args) != Role::Runtime {
        return false;
    }
    let Some(args) = args else {
        return false;
    };
    let watches = args
        .iter()
        .any(|arg| matches!(arg.as_str(), "--watch" | "--reload" | "watch" | "runserver"));
    let tool = args
        .get(1)
        .and_then(|script| crate::command::package_name(script));
    watches || tool.is_some_and(|tool| DEV_SUPERVISORS.contains(&tool.as_str()))
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
    use super::{Role, normalize, role, runs_a_script};

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
