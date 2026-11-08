import {
  agentLines,
  elapsed,
  escapeHtml,
  extraHeight,
  folderName,
  label,
  quotaView,
  rowHeight,
  rowLineCount,
  windowHeight,
} from "./format.js";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { getCurrentWindow } = window.__TAURI__.window;

const card = document.getElementById("card");
const COUNT_ORDER = ["attention", "busy", "idle"];
let state = { sessions: [], compact: false, opacity: 0.9, locked: false, quotas: null, notice: null };
let lastHeight = 0;

function quotaHtml(view) {
  if (!view.bars.length) return "";
  const bars = view.bars.map(({ name, percent, limit }) => `
    <div class="quota ${limit ? "limit" : percent >= 70 ? "high" : ""}">
      <span class="quota-name">${name}</span>
      <span class="quota-bar"><i style="width: ${percent}%"></i></span>
      <span class="quota-percent">${percent}%</span>
    </div>`).join("");
  return `<div class="quotas">${bars}</div>`;
}

function render() {
  const { sessions, compact, opacity, quotas, notice } = state;
  card.classList.toggle("compact", compact);
  card.style.setProperty("--opacity", opacity);

  const counts = COUNT_ORDER
    .map((kind) => [kind, sessions.filter((s) => s.kind === kind).length])
    .filter(([, n]) => n > 0)
    .map(([kind, n]) => `<span class="${kind}" style="--c: var(--${kind})">${n}</span>`)
    .join("");

  const rows = sessions.length
    ? sessions.map((s) => {
        const agents = s.agents ?? [];
        const lines = agentLines(agents).map((line) => `
            <div class="agent"><span>${escapeHtml(line.text)}</span>${line.sinceMs ? `<span>${elapsed(line.sinceMs)}</span>` : ""}</div>`).join("");
        const badge = agents.length ? `<span class="badge">${agents.length}</span>` : "";
        return `
        <div class="row ${s.kind}" style="height: ${rowHeight(compact, rowLineCount(s, compact))}px">
          <i class="dot"></i>
          <div>
            <div class="name">${escapeHtml(s.name)}${badge}</div>
            <div class="sub">${escapeHtml(label(s.kind, s.status))} · ${escapeHtml(folderName(s.cwd))}</div>${lines}
          </div>
          <div class="time">${elapsed(s.sinceMs)}</div>
        </div>`;
      }).join("")
    : `<div class="empty">No active sessions</div>`;

  const view = quotaView(quotas);
  const notes = [...view.notes, ...(notice ? [notice] : [])]
    .map((text) => `<div class="note">${escapeHtml(text)}</div>`)
    .join("");

  card.innerHTML = `<div class="head"><span class="title">CLAUDE</span><span class="counts">${counts}</span></div>${rows}${quotaHtml(view)}${notes}`;

  const height = windowHeight(sessions, compact, extraHeight(view, notice));
  if (height !== lastHeight) {
    lastHeight = height;
    invoke("fit", { height });
  }
}

document.addEventListener("mousedown", (e) => {
  if (e.button === 0 && !state.locked) getCurrentWindow().startDragging();
});

document.addEventListener("contextmenu", (e) => {
  e.preventDefault();
  invoke("show_menu");
});

await listen("state", (event) => {
  state = event.payload;
  render();
});

state = await invoke("get_state");
render();
setInterval(render, 1000);
