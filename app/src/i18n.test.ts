import { describe, expect, it } from "vitest";

import { localeOf, translator } from "./i18n";

const MIB = 1024 * 1024;
const GIB = 1024 * MIB;

describe("translator", () => {
  it("shows megabytes, then gigabytes, the way the language writes numbers", () => {
    expect(translator("en").bytes(812 * MIB)).toBe("812 MB");
    expect(translator("en").bytes(2.2 * GIB)).toBe("2.2 GB");
    expect(translator("tr").bytes(2.2 * GIB)).toBe("2,2 GB");
  });

  it("shows uptime with its two largest units", () => {
    const { uptime } = translator("en");
    expect(uptime(20)).toBe("<1m");
    expect(uptime(41 * 60)).toBe("41m");
    expect(uptime(3 * 3600 + 12 * 60)).toBe("3h 12m");
    expect(uptime(2 * 86400 + 5 * 3600)).toBe("2d 5h");
    expect(translator("tr").uptime(3 * 3600 + 12 * 60)).toBe("3 sa 12 dk");
  });

  it("puts the percent sign where the language does", () => {
    expect(translator("en").percent(5.4)).toBe("5%");
    expect(translator("tr").percent(5.4)).toBe("%5");
  });

  it("picks the plural form by count", () => {
    expect(translator("en").plural("servers", 1)).toBe("1 server");
    expect(translator("en").plural("servers", 3)).toBe("3 servers");
    expect(translator("tr").plural("servers", 3)).toBe("3 sunucu");
  });

  it("speaks Turkish only to Turkish systems", () => {
    expect(localeOf("tr-TR")).toBe("tr");
    expect(localeOf("en-GB")).toBe("en");
    expect(localeOf("de")).toBe("en");
  });
});
