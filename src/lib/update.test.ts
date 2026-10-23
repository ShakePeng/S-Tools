import { describe, expect, it } from "vitest";
import {
  downloadProgressLabel,
  effectiveUpdateTotal,
  formatByteSize,
  updatePercent,
} from "./update";

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

  it("prefers response content length over manifest size", () => {
    expect(effectiveUpdateTotal(2048, 4096)).toBe(2048);
  });

  it("falls back to manifest size when content length is missing", () => {
    expect(effectiveUpdateTotal(null, 4096)).toBe(4096);
    expect(effectiveUpdateTotal(undefined, 4096)).toBe(4096);
    expect(effectiveUpdateTotal(0, 4096)).toBe(4096);
  });

  it("returns null when both size sources are absent", () => {
    expect(effectiveUpdateTotal(null, null)).toBeNull();
  });
});
