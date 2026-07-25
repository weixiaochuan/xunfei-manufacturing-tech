export function stripHtml(value: string | null | undefined): string {
  if (!value) return "";
  return value.replace(/<[^>]*>/g, "").replace(/&nbsp;/g, " ").trim();
}

export function relativeTime(value: string | number | Date | null | undefined): string {
  if (!value) return "";
  const time = new Date(value).getTime();
  if (!Number.isFinite(time)) return String(value);
  const diff = Date.now() - time;
  const minute = 60 * 1000;
  const hour = 60 * minute;
  const day = 24 * hour;
  if (diff < minute) return "刚刚";
  if (diff < hour) return `${Math.floor(diff / minute)} 分钟前`;
  if (diff < day) return `${Math.floor(diff / hour)} 小时前`;
  return `${Math.floor(diff / day)} 天前`;
}
