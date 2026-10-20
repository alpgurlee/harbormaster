import type { MouseEvent as ReactMouseEvent } from "react";

/** How far, in pixels, the pointer goes before a press on a header drags the window. */
const THRESHOLD = 4;

/**
 * A press on a header that drags the window once the pointer has moved a
 * little, so a click there stays a click. A press on its controls is theirs.
 */
export function dragsAfterMoving(onDrag: () => void) {
  return (event: ReactMouseEvent) => {
    if (event.button !== 0 || (event.target as Element).closest("button, input, select, a")) return;
    const start = { x: event.screenX, y: event.screenY };
    const move = (moved: MouseEvent) => {
      // The button may have been let go where this window couldn't see it.
      if ((moved.buttons & 1) === 0) {
        stop();
        return;
      }
      if (Math.hypot(moved.screenX - start.x, moved.screenY - start.y) < THRESHOLD) return;
      stop();
      onDrag();
    };
    const stop = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", stop);
      window.removeEventListener("blur", stop);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", stop);
    window.addEventListener("blur", stop);
  };
}
