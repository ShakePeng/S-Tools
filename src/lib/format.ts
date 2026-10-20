export function formatDate(timestamp: number): string {
  const date = new Date(timestamp * 1000);
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(date);
}

export function truncate(value: string, max = 80): string {
  const clean = value.replace(/\s+/g, " ").trim();
  return clean.length > max ? `${clean.slice(0, max)}…` : clean;
}

export function isCjkText(value: string): boolean {
  return /[\u3400-\u9fff]/.test(value);
}

export function inferTargetLanguage(value: string, configured: string): string {
  if (configured != "auto") return configured;
  return isCjkText(value) ? "en" : "zh-CN";
}
