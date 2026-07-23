export function formatFileSize(sizeBytes: number): string {
  if (!Number.isFinite(sizeBytes) || sizeBytes < 0) {
    return "—";
  }
  if (sizeBytes < 1024) {
    return `${Math.floor(sizeBytes)} B`;
  }
  if (sizeBytes < 1024 * 1024) {
    return `${formatUnit(sizeBytes / 1024)} KiB`;
  }
  return `${formatUnit(sizeBytes / (1024 * 1024))} MiB`;
}

function formatUnit(value: number): string {
  return value >= 10 || Number.isInteger(value) ? value.toFixed(0) : value.toFixed(1);
}
