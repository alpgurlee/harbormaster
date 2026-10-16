/** A rectangle, from its top left corner. */
export interface Box {
  x: number;
  y: number;
  width: number;
  height: number;
}

/**
 * A box inside `space` for each of `sizes`, in the same order: each as
 * large as its share and as near square as rows allow, the largest first
 * from the top left. This is the squarified treemap of Bruls, Huizing and
 * van Wijk: a row grows along the shorter side for as long as that makes
 * its worst box squarer, then the rest of the space is filled the same way.
 */
export function squarify(sizes: number[], space: Box): Box[] {
  const boxes: Box[] = sizes.map(() => ({ x: space.x, y: space.y, width: 0, height: 0 }));
  const total = sizes.reduce((sum, size) => sum + Math.max(0, size), 0);
  if (total <= 0 || space.width <= 0 || space.height <= 0) return boxes;
  const scale = (space.width * space.height) / total;
  const order = sizes
    .map((size, index) => ({ index, area: Math.max(0, size) * scale }))
    .filter(({ area }) => area > 0)
    .sort((a, b) => b.area - a.area);
  let rest = { ...space };
  let start = 0;
  while (start < order.length) {
    const side = Math.min(rest.width, rest.height);
    let end = start + 1;
    let row = order[start].area;
    while (end < order.length) {
      const longer = row + order[end].area;
      if (worst(order[start].area, order[end].area, longer, side) > worst(order[start].area, order[end - 1].area, row, side)) {
        break;
      }
      row = longer;
      end += 1;
    }
    rest = laid(order.slice(start, end), row, rest, boxes);
    start = end;
  }
  return boxes;
}

/** The least square box of a row summing to `row` along `side`, whose largest and smallest are given. */
function worst(largest: number, smallest: number, row: number, side: number): number {
  const across = side * side;
  return Math.max((across * largest) / (row * row), (row * row) / (across * smallest));
}

/** Lays `items`, summing to `row`, along the shorter side of `rest`, and gives back what is left of it. */
function laid(items: { index: number; area: number }[], row: number, rest: Box, boxes: Box[]): Box {
  if (rest.width >= rest.height) {
    // A column down the left side.
    const width = row / rest.height;
    let y = rest.y;
    for (const { index, area } of items) {
      const height = area / width;
      boxes[index] = { x: rest.x, y, width, height };
      y += height;
    }
    return { x: rest.x + width, y: rest.y, width: rest.width - width, height: rest.height };
  }
  // A row across the top.
  const height = row / rest.width;
  let x = rest.x;
  for (const { index, area } of items) {
    const width = area / height;
    boxes[index] = { x, y: rest.y, width, height };
    x += width;
  }
  return { x: rest.x, y: rest.y + height, width: rest.width, height: rest.height - height };
}
