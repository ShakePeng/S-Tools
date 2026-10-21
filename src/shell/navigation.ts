import type { AppSettings } from "../types";

export type Section = "overview" | "translation" | "screenshot" | "settings";

export interface SectionItem {
  id: Section;
  label: string;
  icon: string;
}

export const shellSections: SectionItem[] = [
  { id: "overview", label: "概览", icon: "home" },
  { id: "translation", label: "翻译", icon: "translate" },
  { id: "screenshot", label: "截图", icon: "screenshot" },
  { id: "settings", label: "设置", icon: "settings" },
];

const sectionAliases: Record<string, Section> = {
  home: "overview",
  history: "overview",
  updates: "settings",
  providers: "translation",
};

export function normalizeSection(value: string | null | undefined): Section | null {
  if (!value) return null;
  const normalized = sectionAliases[value] || value;
  return shellSections.some((item) => item.id == normalized) ? (normalized as Section) : null;
}

export function isSectionVisible(
  id: Section,
  settings: Pick<AppSettings, "translationEnabled" | "screenshotEnabled">,
): boolean {
  if (id == "translation") return settings.translationEnabled;
  if (id == "screenshot") return settings.screenshotEnabled;
  return true;
}

export function visibleSectionItems(
  settings: Pick<AppSettings, "translationEnabled" | "screenshotEnabled">,
): SectionItem[] {
  return shellSections.filter((item) => isSectionVisible(item.id, settings));
}

export function resolveRequestedSection(
  value: string | null | undefined,
  settings: Pick<AppSettings, "translationEnabled" | "screenshotEnabled">,
): Section {
  const requested = normalizeSection(value);
  return requested && isSectionVisible(requested, settings) ? requested : "overview";
}

export function resolveInitialSection(
  requested: string | null | undefined,
  remembered: string | null | undefined,
  settings: Pick<AppSettings, "translationEnabled" | "screenshotEnabled">,
): Section {
  return resolveRequestedSection(requested || remembered, settings);
}
