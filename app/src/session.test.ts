import { expect, it } from "vitest";

import { agentOf } from "./session";
import type { Service } from "./types";

it("names only real agents, whatever the processes above are called", () => {
  const service = {
    ancestors: [
      { pid: 3, name: "constructor" },
      { pid: 2, name: "__proto__" },
      { pid: 1, name: "claude" },
    ],
  } as Service;

  expect(agentOf(service)).toBe("Claude Code");
});
