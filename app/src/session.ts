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

/** The nearest coding agent the server was started under, if any. */
export function agentOf(service: Service): string | null {
  for (const ancestor of service.ancestors) {
    const agent = AGENTS.get(ancestor.name.toLowerCase());
    if (agent) return agent;
  }
  return null;
}
