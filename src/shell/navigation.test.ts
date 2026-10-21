import { describe, expect, it } from "vitest";
import {
  normalizeSection,
  resolveInitialSection,
  resolveRequestedSection,
  visibleSectionItems,
} from "./navigation";

const disabledPlugins = {
  translationEnabled: false,
  screenshotEnabled: false,
};

describe("shell navigation", () => {
  it("maps legacy section names", () => {
    expect(normalizeSection("home")).toBe("overview");
    expect(normalizeSection("history")).toBe("overview");
    expect(normalizeSection("providers")).toBe("translation");
    expect(normalizeSection("updates")).toBe("settings");
    expect(normalizeSection("unknown")).toBeNull();
  });

  it("keeps overview and settings visible when plugins are disabled", () => {
    expect(visibleSectionItems(disabledPlugins).map((item) => item.id)).toEqual([
      "overview",
      "settings",
    ]);
  });

  it("only opens enabled plugin sections", () => {
    expect(resolveRequestedSection("translation", disabledPlugins)).toBe("overview");
    expect(
      resolveRequestedSection("translation", {
        translationEnabled: true,
        screenshotEnabled: false,
      }),
    ).toBe("translation");
    expect(resolveRequestedSection("updates", disabledPlugins)).toBe("settings");
  });

  it("restores the remembered section unless an explicit section is requested", () => {
    const enabledPlugins = {
      translationEnabled: true,
      screenshotEnabled: true,
    };

    expect(resolveInitialSection(null, "screenshot", enabledPlugins)).toBe("screenshot");
    expect(resolveInitialSection("settings", "screenshot", enabledPlugins)).toBe("settings");
    expect(resolveInitialSection(null, "translation", disabledPlugins)).toBe("overview");
  });
});
