import { expect, it } from "vitest";

import { urlOf } from "./address";

it("points at a listener where it answers, as the app opens it", () => {
  expect(urlOf({ port: 3000, address: "127.0.0.1" })).toBe("http://localhost:3000");
  expect(urlOf({ port: 5173, address: "::" })).toBe("http://localhost:5173");
  expect(urlOf({ port: 8080, address: "192.168.1.5" })).toBe("http://192.168.1.5:8080");
  expect(urlOf({ port: 8080, address: "fd00::5" })).toBe("http://[fd00::5]:8080");
});
