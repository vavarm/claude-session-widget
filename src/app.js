import {
  agentLines,
  contextLevel,
  elapsed,
  escapeHtml,
  folderName,
  infoTitle,
  label,
  layout,
  rowHeight,
  rowLineCount,
} from "./format.js";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { getCurrentWindow } = window.__TAURI__.window;

const card = document.getElementById("card");
const COUNT_ORDER = ["attention", "busy", "idle"];
let state = { sessions: [], compact: false, opacity: 0.9, locked: false, quotas: null, notice: null };
let hover = false;
let lastHeight = 0;

function quotaHtml(quota) {
  if (!quota.bars.length) return "";
  const bars = quota.bars.map(({ name, percent, limit }) => `
    <div class="quota ${limit ? "limit" : percent >= 70 ? "high" : ""}"${percent == null ? ` title="No current figure: Claude Code only reports it in terminal sessions"` : ""}>
      <span class="quota-name">${name}</span>
      <span class="quota-bar"><i style="width: ${percent ?? 0}%"></i></span>
      <span class="quota-percent">${percent == null ? "\u2013" : `${percent}%`}</span>
    </div>`).join("");
  return `<div class="quotas">${bars}</div>`;
}

function rowHtml(s, compact) {
  const agents = s.agents ?? [];
  const lines = agentLines(agents).map((line) => `
            <div class="agent"><span>${escapeHtml(line.text)}</span>${line.sinceMs ? `<span>${elapsed(line.sinceMs)}</span>` : ""}</div>`).join("");
  const badge = agents.length ? `<span class="badge">${agents.length}</span>` : "";
  const context = s.info?.contextPercent;
  const bar = context == null ? "" : `<i class="ctx ${contextLevel(context)}"><b style="width: ${Math.min(context, 100)}%"></b></i>`;
  const title = [infoTitle(s.info), "Double-click to open"].filter(Boolean).join("\n");
  return `
        <div class="row ${s.kind}" data-pid="${s.pid}" title="${escapeHtml(title)}" style="height: ${rowHeight(compact, rowLineCount(s, compact))}px">
          <i class="dot"></i>
          <div>
            <div class="name">${escapeHtml(s.name)}${badge}</div>
            <div class="sub">${escapeHtml(label(s.kind, s.status))} · ${escapeHtml(folderName(s.cwd))}</div>${lines}
          </div>
          <div class="time">${elapsed(s.sinceMs)}</div>${bar}
        </div>`;
}

function render() {
  const { sessions, compact, opacity, notice } = state;
  card.classList.toggle("compact", compact);
  card.style.setProperty("--opacity", opacity);

  const { shown, folded, collapsed, quota, height } = layout(state, hover);

  const counts = COUNT_ORDER
    .map((kind) => [kind, sessions.filter((s) => s.kind === kind).length])
    .filter(([, n]) => n > 0)
    .map(([kind, n]) => `<span class="${kind}" style="--c: var(--${kind})">${n}</span>`)
    .join("");
  const head = `<div class="head"><span class="title">CLAUDE</span><span class="counts">${counts}</span></div>`;

  if (collapsed) {
    card.innerHTML = head;
  } else {
    const rows = shown.length
      ? shown.map((s) => rowHtml(s, compact)).join("")
      : folded.length ? "" : `<div class="empty">No active sessions</div>`;
    const foldedRow = folded.length ? `<div class="folded">+${folded.length} idle for over an hour</div>` : "";
    const notes = [...quota.notes, ...(notice ? [notice] : [])]
      .map((text) => `<div class="note">${escapeHtml(text)}</div>`)
      .join("");
    card.innerHTML = `${head}${rows}${foldedRow}${quotaHtml(quota)}${notes}`;
  }

  if (height !== lastHeight) {
    lastHeight = height;
    invoke("fit", { height });
  }
}

document.addEventListener("mousedown", (e) => {
  if (e.button === 0 && !state.locked) getCurrentWindow().startDragging();
});

document.addEventListener("dblclick", (e) => {
  const row = e.target.closest?.(".row");
  if (row) invoke("open_session", { pid: Number(row.dataset.pid) });
});

document.addEventListener("contextmenu", (e) => {
  e.preventDefault();
  invoke("show_menu");
});

document.documentElement.addEventListener("mouseenter", () => { hover = true; render(); });
document.documentElement.addEventListener("mouseleave", () => { hover = false; render(); });

await listen("state", (event) => {
  state = event.payload;
  render();
});

state = await invoke("get_state");
render();
setInterval(render, 1000);
