export const HEADER_HEIGHT = 34;
export const FOOTER_HEIGHT = 8;
export const AGENT_LINE_HEIGHT = 16;
export const MAX_AGENT_LINES = 3;

export const rowHeight = (compact, agentLines = 0) => (compact ? 28 : 46 + agentLines * AGENT_LINE_HEIGHT);

// One line per running agent or background task, with the rest folded into a last line.
export function agentLines(agents, now = Date.now()) {
  const text = (task) => {
    const what = task.description || task.name || task.kind || "task";
    return task.agentType ? `${task.agentType}: ${what}` : what;
  };
  if (agents.length <= MAX_AGENT_LINES) {
    return agents.map((task) => ({ text: text(task), sinceMs: task.startTime }));
  }
  const shown = agents.slice(0, MAX_AGENT_LINES - 1).map((task) => ({ text: text(task), sinceMs: task.startTime }));
  return [...shown, { text: `+${agents.length - shown.length} more`, sinceMs: null }];
}

export function rowLineCount(session, compact) {
  return compact ? 0 : agentLines(session.agents ?? []).length;
}

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

export function windowHeight(sessions, compact, extra = 0, emptyRow = true) {
  const rows = sessions.length
    ? sessions.reduce((sum, s) => sum + rowHeight(compact, rowLineCount(s, compact)), 0)
    : emptyRow ? rowHeight(compact) : 0;
  return HEADER_HEIGHT + rows + extra + FOOTER_HEIGHT;
}

export const FOLD_AFTER_MS = 3_600_000;
export const FOLDED_HEIGHT = 28;
export const CONTEXT_WARNING = 80;
export const CONTEXT_CRITICAL = 95;

// Decides what the widget shows: which sessions, whether it shrinks to its header, and its height.
export function layout(state, hover, now = Date.now()) {
  const { sessions, compact, foldIdle, autoCollapse, quotas, notice } = state;
  const folded = foldIdle ? sessions.filter((s) => s.kind === "idle" && now - s.sinceMs > FOLD_AFTER_MS) : [];
  const shown = sessions.filter((s) => !folded.includes(s));
  const quiet = sessions.every((s) => s.kind !== "busy" && s.kind !== "attention");
  const collapsed = Boolean(autoCollapse) && quiet && !hover;
  const quota = quotaView(quotas, now / 1000);
  const extra = extraHeight(quota, notice) + (folded.length ? FOLDED_HEIGHT : 0);
  const height = collapsed
    ? HEADER_HEIGHT + FOOTER_HEIGHT
    : windowHeight(shown, compact, extra, folded.length === 0);
  return { shown, folded, collapsed, quota, height };
}

export function contextLevel(percent) {
  if (percent >= CONTEXT_CRITICAL) return "critical";
  return percent >= CONTEXT_WARNING ? "warning" : "";
}

export function infoTitle(info) {
  if (!info) return "";
  const parts = [];
  if (info.model) parts.push(info.model);
  if (info.contextPercent != null) parts.push(`context ${Math.round(info.contextPercent)} %`);
  if (info.costUsd) parts.push(`$${info.costUsd.toFixed(2)}`);
  return parts.join(" · ");
}

export const LIMIT_PERCENT = 90;
export const STALE_QUOTA_MS = 15 * 60 * 1000;
export const QUOTA_BARS_HEIGHT = 26;
export const NOTE_HEIGHT = 15;

export function untilReset(resetsAt, now = Date.now() / 1000) {
  const minutes = Math.max(Math.ceil((resetsAt - now) / 60), 1);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h${String(minutes % 60).padStart(2, "0")}`;
  return `${Math.floor(hours / 24)}d${String(hours % 24).padStart(2, "0")}h`;
}

const QUOTA_WINDOWS = [["fiveHour", "5h"], ["sevenDay", "7d"]];

// Bars for each known window; a window at the limit also gets a line saying when it resets.
export function quotaView(quotas, now = Date.now() / 1000) {
  const bars = [];
  const notes = [];
  for (const [key, name] of QUOTA_WINDOWS) {
    const window = quotas?.[key];
    if (!window) continue;
    const percent = Math.min(Math.max(Math.round(window.usedPercentage), 0), 100);
    const limit = percent >= LIMIT_PERCENT;
    bars.push({ name, percent, limit });
    if (limit) notes.push(`${name} limit: resets in ${untilReset(window.resetsAt, now)}`);
  }
  if (bars.length && quotas.asOf && now * 1000 - quotas.asOf > STALE_QUOTA_MS) {
    const time = new Date(quotas.asOf).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    notes.push(`usage as of ${time}`);
  }
  return { bars, notes };
}

export function extraHeight(view, notice) {
  const lines = view.notes.length + (notice ? 1 : 0);
  return (view.bars.length ? QUOTA_BARS_HEIGHT : 0) + lines * NOTE_HEIGHT;
}

export function escapeHtml(text) {
  return text.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
}
