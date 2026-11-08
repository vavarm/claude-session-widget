export const HEADER_HEIGHT = 34;
export const FOOTER_HEIGHT = 8;
export const rowHeight = (compact) => (compact ? 28 : 46);

export function elapsed(sinceMs, now = Date.now()) {
  const seconds = Math.max(Math.floor((now - sinceMs) / 1000), 0);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h${String(minutes % 60).padStart(2, "0")}`;
  return `${Math.floor(hours / 24)}d`;
}

export function label(kind, raw) {
  switch (kind) {
    case "busy": return "Busy";
    case "idle": return "Idle";
    case "attention": return "Needs you";
    default: return raw || "Unknown";
  }
}

export function folderName(cwd) {
  return cwd.replace(/[\\/]+$/, "").split(/[\\/]/).pop() ?? "";
}

export function windowHeight(sessionCount, compact) {
  return HEADER_HEIGHT + Math.max(sessionCount, 1) * rowHeight(compact) + FOOTER_HEIGHT;
}

export function escapeHtml(text) {
  return text.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
}
