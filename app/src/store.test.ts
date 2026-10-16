import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";

import { publish, useUpdate } from "./store";

afterEach(cleanup);

it("hands every new scan to the components that read it", () => {
  const { result } = renderHook(() => useUpdate());
  expect(result.current).toBeNull();

  act(() => publish({ services: [], memory: { used: 1, total: 2 }, apps: null, scannedAt: 42 }));

  expect(result.current?.scannedAt).toBe(42);
});
