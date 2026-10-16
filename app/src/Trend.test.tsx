import { cleanup, render } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";

import { Trend } from "./Trend";

afterEach(cleanup);

function points(values: number[]) {
  const { container } = render(<Trend values={values} />);
  const line = container.querySelector("polyline")?.getAttribute("points");
  return line?.split(" ").map((point) => point.split(",").map(Number)) ?? null;
}

it("draws memory as a line that climbs when it grows", () => {
  const line = points([100, 100, 200]);

  expect(line).toHaveLength(3);
  expect(line?.[2][1]).toBeLessThan(line?.[0][1] ?? 0);
});

it("keeps a change under a tenth of the peak from looking like a climb", () => {
  const line = points([1000, 1090]);

  expect(line).toHaveLength(2);
  expect(Math.abs((line?.[1][1] ?? 0) - (line?.[0][1] ?? 0))).toBeLessThan(1.2);
});

it("draws a real climb across the whole height", () => {
  const line = points([1000, 2000]);

  expect((line?.[0][1] ?? 0) - (line?.[1][1] ?? 0)).toBeGreaterThan(10);
});

it("draws nothing until there are two samples", () => {
  expect(points([100])).toBeNull();
});
