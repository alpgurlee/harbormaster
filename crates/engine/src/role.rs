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

/// A shell that runs a command or a script and exits, as package managers do
/// with `sh -c "next dev"` or `cmd /d /s /c "next dev"`. Interactive shells
/// carry at most option flags.
pub(crate) fn runs_a_script(args: Option<&[String]>) -> bool {
    let Some(args) = args else {
        return false;
    };
    args.iter().skip(1).any(|arg| {
        let lower = arg.to_lowercase();
        match lower.as_str() {
            "-c" | "/c" | "-command" | "-file" => true,
            "/k" | "-noexit" => false,
            _ if lower.starts_with("--") => false,
            // Bundled POSIX flags such as -lc or -ilc.
            _ if lower.starts_with('-') => lower.len() <= 4 && lower.contains('c'),
            // cmd switches such as /d, /s, /q.
            _ if lower.starts_with('/') && lower.len() <= 3 => false,
            // A command string or a script path.
            _ => true,
        }
    })
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
