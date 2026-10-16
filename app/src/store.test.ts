import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";

import { publish, trendOf, useUpdate } from "./store";
import type { Service } from "./types";

afterEach(cleanup);

it("hands every new scan to the components that read it", () => {
  const { result } = renderHook(() => useUpdate());
  expect(result.current).toBeNull();

  act(() => publish({ services: [], memory: { used: 1, total: 2 }, apps: null, scannedAt: 42 }));

  expect(result.current?.scannedAt).toBe(42);
});

it("remembers each server's memory for ten minutes, and forgets a server that stopped", () => {
  const server = (memoryBytes: number) => ({ id: "20-1", memoryBytes }) as Service;
  const at = (minutes: number, services: Service[]) =>
    act(() => publish({ services, memory: null, apps: null, scannedAt: minutes * 60_000 }));

  at(0, [server(100)]);
  at(5, [server(150)]);
  at(11, [server(120)]);
  expect(trendOf("20-1")).toEqual([150, 120]);

  at(12, []);
  expect(trendOf("20-1")).toEqual([]);
});

it("lets a trend lapse while the server's memory can't be read", () => {
  const server = (memoryBytes: number | null) => ({ id: "30-1", memoryBytes }) as Service;
  const at = (minutes: number, services: Service[]) =>
    act(() => publish({ services, memory: null, apps: null, scannedAt: minutes * 60_000 }));

  at(0, [server(100)]);
  at(1, [server(110)]);
  at(12, [server(null)]);

  expect(trendOf("30-1")).toEqual([]);
});
