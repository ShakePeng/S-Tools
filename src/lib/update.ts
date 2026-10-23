const BYTE_UNITS = ["B", "KB", "MB", "GB"];

export function formatByteSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < BYTE_UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const digits = unit == 0 ? 0 : value >= 100 ? 0 : 1;
  return `${value.toFixed(digits).replace(/\.0$/, "")} ${BYTE_UNITS[unit]}`;
}

export function updatePercent(downloaded: number, total: number | null): number | null {
  if (!total || !Number.isFinite(total) || total <= 0) return null;
  const percent = Math.round((Math.max(0, downloaded) / total) * 100);
  return Math.min(100, Math.max(0, percent));
}

export function effectiveUpdateTotal(
  contentLength: number | null | undefined,
  manifestSize: number | null | undefined,
): number | null {
  if (contentLength && Number.isFinite(contentLength) && contentLength > 0) {
    return contentLength;
  }
  if (manifestSize && Number.isFinite(manifestSize) && manifestSize > 0) {
    return manifestSize;
  }
  return null;
}

export function downloadProgressLabel(downloaded: number, total: number | null): string {
  if (total && total > 0) {
    return `${formatByteSize(downloaded)} / ${formatByteSize(total)}`;
  }
  return `已接收 ${formatByteSize(downloaded)}`;
}
