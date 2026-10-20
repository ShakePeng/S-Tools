import { describe, expect, it } from "vitest";
import { inferTargetLanguage, isCjkText, truncate } from "./format";

describe("format helpers", () => {
  it("detects CJK text", () => {
    expect(isCjkText("今天天气不错")).toBe(true);
    expect(isCjkText("Hello world")).toBe(false);
  });

  it("infers a useful target language", () => {
    expect(inferTargetLanguage("你好", "auto")).toBe("en");
    expect(inferTargetLanguage("Hello", "auto")).toBe("zh-CN");
    expect(inferTargetLanguage("Hello", "ja")).toBe("ja");
  });

  it("truncates long values", () => {
    expect(truncate("abcdef", 3)).toBe("abc…");
  });
});
