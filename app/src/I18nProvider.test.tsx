import { cleanup, render } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";

import { I18nProvider } from "./i18n";

afterEach(cleanup);

it("marks the document with the language it speaks", () => {
  render(<I18nProvider locale="tr">x</I18nProvider>);

  expect(document.documentElement.lang).toBe("tr");
});
