import { fireEvent } from "@testing-library/react";
import type { MouseEvent as ReactMouseEvent } from "react";
import { describe, expect, it, vi } from "vitest";

import { dragsAfterMoving } from "./drag";

function press(target: Element, button = 0) {
  return { button, target, screenX: 100, screenY: 100 } as unknown as ReactMouseEvent;
}

describe("dragging a header", () => {
  it("starts once the pointer has moved, and only once", () => {
    const onDrag = vi.fn();
    dragsAfterMoving(onDrag)(press(document.body));

    fireEvent.mouseMove(window, { buttons: 1, screenX: 102, screenY: 101 });
    expect(onDrag).not.toHaveBeenCalled();
    fireEvent.mouseMove(window, { buttons: 1, screenX: 110, screenY: 100 });
    fireEvent.mouseMove(window, { buttons: 1, screenX: 130, screenY: 100 });
    expect(onDrag).toHaveBeenCalledTimes(1);
  });

  it("forgets a press once the button was let go elsewhere, or the window lost focus", () => {
    const onDrag = vi.fn();
    const drags = dragsAfterMoving(onDrag);

    drags(press(document.body));
    fireEvent.mouseMove(window, { buttons: 0, screenX: 150, screenY: 100 });
    drags(press(document.body));
    fireEvent.blur(window);
    fireEvent.mouseMove(window, { buttons: 1, screenX: 150, screenY: 100 });

    expect(onDrag).not.toHaveBeenCalled();
  });

  it("leaves a click a click", () => {
    const onDrag = vi.fn();
    dragsAfterMoving(onDrag)(press(document.body));

    fireEvent.mouseUp(window);
    fireEvent.mouseMove(window, { buttons: 1, screenX: 150, screenY: 100 });

    expect(onDrag).not.toHaveBeenCalled();
  });

  it("leaves a header's buttons, and other mouse buttons, alone", () => {
    const onDrag = vi.fn();
    const button = document.createElement("button");
    const icon = button.appendChild(document.createElement("span"));
    document.body.appendChild(button);
    dragsAfterMoving(onDrag)(press(icon));
    dragsAfterMoving(onDrag)(press(document.body, 2));

    fireEvent.mouseMove(window, { buttons: 1, screenX: 150, screenY: 100 });

    expect(onDrag).not.toHaveBeenCalled();
    button.remove();
  });
});
