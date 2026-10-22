import { describe, expect, it } from "vitest";
import { downloadProgressLabel, formatByteSize, updatePercent } from "./update";

describe("update progress formatting", () => {
  it("formats downloaded bytes with stable units", () => {
    expect(formatByteSize(0)).toBe("0 B");
    expect(formatByteSize(1536)).toBe("1.5 KB");
    expect(formatByteSize(3.7 * 1024 * 1024)).toBe("3.7 MB");
  });

  it("calculates and clamps determinate percentages", () => {
    expect(updatePercent(0, 100)).toBe(0);
    expect(updatePercent(42, 100)).toBe(42);
    expect(updatePercent(120, 100)).toBe(100);
  });

  it("returns null when the total size is unknown", () => {
    expect(updatePercent(1024, null)).toBeNull();
    expect(updatePercent(1024, 0)).toBeNull();
    expect(downloadProgressLabel(1024, null)).toBe("已接收 1 KB");
  });
});
