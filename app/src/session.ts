import type { Service } from "./types";

/** Coding agents by the command that starts them, as the engine names them. */
const AGENTS = new Map([
  ["claude", "Claude Code"],
  ["codex", "Codex"],
  ["gemini", "Gemini CLI"],
  ["opencode", "opencode"],
  ["aider", "Aider"],
  ["cursor-agent", "Cursor Agent"],
]);

/** Terminals by the process name the engine reports, lowercased. */
const TERMINALS = new Map([
  ["terminal", "Terminal"],
  ["iterm2", "iTerm2"],
  ["ghostty", "Ghostty"],
  ["wezterm-gui", "WezTerm"],
  ["kitty", "kitty"],
  ["alacritty", "Alacritty"],
  ["warp", "Warp"],
  ["hyper", "Hyper"],
  ["tabby", "Tabby"],
  ["windowsterminal", "Windows Terminal"],
  ["code", "VS Code"],
  ["code helper", "VS Code"],
  ["cursor", "Cursor"],
  ["cursor helper", "Cursor"],
  ["zed", "Zed"],
]);

const SHELLS = new Set(["sh", "bash", "zsh", "fish", "dash", "ksh", "csh", "tcsh", "nu", "cmd", "powershell", "pwsh"]);

/** `-zsh` for a login shell, `pwsh.exe` on Windows: the name itself. */
function bare(name: string): string {
  return name.toLowerCase().replace(/^-/, "").replace(/\.exe$/, "");
}

/** The terminal and shell the server was started from: `iTerm2 · zsh`. */
export function terminalOf(service: Service): string | null {
  const names = service.ancestors.map((ancestor) => bare(ancestor.name));
  const terminal = names.map((name) => TERMINALS.get(name)).find(Boolean);
  const shell = names.find((name) => SHELLS.has(name));
  const parts = [terminal, shell].filter((part): part is string => Boolean(part));
  return parts.length > 0 ? parts.join(" · ") : null;
}

/** The nearest coding agent the server was started under, if any. */
export function agentOf(service: Service): string | null {
  for (const ancestor of service.ancestors) {
    const agent = AGENTS.get(ancestor.name.toLowerCase());
    if (agent) return agent;
  }
  return null;
}

/** How the panel names a process above the server: `Claude Code`, `iTerm2`, `zsh`. */
export function displayName(name: string): string {
  const key = bare(name);
  return AGENTS.get(key) ?? TERMINALS.get(key) ?? name.replace(/^-/, "").replace(/\.exe$/i, "");
}
