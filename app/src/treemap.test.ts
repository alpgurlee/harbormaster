import { describe, expect, it } from "vitest";

import { squarify, type Box } from "./treemap";

const area = (box: Box) => box.width * box.height;

function overlap(a: Box, b: Box): number {
  const width = Math.min(a.x + a.width, b.x + b.width) - Math.max(a.x, b.x);
  const height = Math.min(a.y + a.height, b.y + b.height) - Math.max(a.y, b.y);
  return width > 0 && height > 0 ? width * height : 0;
}

const aspect = (box: Box) => Math.max(box.width / box.height, box.height / box.width);

describe("a squarified treemap", () => {
  const space: Box = { x: 10, y: 20, width: 600, height: 400 };
  const sizes = [6, 6, 4, 3, 2, 2, 1];

  it("gives each its share of the space, inside it and over no other", () => {
    const boxes = squarify(sizes, space);
    const total = sizes.reduce((sum, size) => sum + size, 0);

    boxes.forEach((box, index) => {
      expect(area(box)).toBeCloseTo((area(space) * sizes[index]) / total, 6);
      expect(box.x).toBeGreaterThanOrEqual(space.x - 1e-9);
      expect(box.y).toBeGreaterThanOrEqual(space.y - 1e-9);
      expect(box.x + box.width).toBeLessThanOrEqual(space.x + space.width + 1e-9);
      expect(box.y + box.height).toBeLessThanOrEqual(space.y + space.height + 1e-9);
      boxes.slice(index + 1).forEach((other) => expect(overlap(box, other)).toBeCloseTo(0, 6));
    });
  });

  it("keeps the boxes near square", () => {
    const boxes = squarify(sizes, space);

    expect(Math.max(...boxes.map(aspect))).toBeLessThan(3);
  });

  it("answers in the order it was asked, whatever the sizes", () => {
    const [small, large] = squarify([1, 3], space);

    expect(area(large)).toBeCloseTo(3 * area(small), 6);
  });

  it("gives nothing to what takes no room, and all of it to one alone", () => {
    expect(squarify([5], space)).toEqual([space]);
    expect(squarify([0, 0], space).map(area)).toEqual([0, 0]);
    expect(squarify([4, 0], space).map(area)).toEqual([area(space), 0]);
    expect(squarify([], space)).toEqual([]);
  });
});
