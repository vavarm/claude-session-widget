import { elapsed, escapeHtml, folderName, label, windowHeight } from "./format.js";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { getCurrentWindow } = window.__TAURI__.window;

const card = document.getElementById("card");
const COUNT_ORDER = ["attention", "busy", "idle"];
let state = { sessions: [], compact: false, opacity: 0.9 };
let lastHeight = 0;

function render() {
  const { sessions, compact, opacity } = state;
  card.classList.toggle("compact", compact);
  card.style.setProperty("--opacity", opacity);

  const counts = COUNT_ORDER
    .map((kind) => [kind, sessions.filter((s) => s.kind === kind).length])
    .filter(([, n]) => n > 0)
    .map(([kind, n]) => `<span class="${kind}" style="--c: var(--${kind})">${n}</span>`)
    .join("");

  const rows = sessions.length
    ? sessions.map((s) => `
        <div class="row ${s.kind}">
          <i class="dot"></i>
          <div>
            <div class="name">${escapeHtml(s.name)}</div>
            <div class="sub">${escapeHtml(label(s.kind, s.status))} · ${escapeHtml(folderName(s.cwd))}</div>
          </div>
          <div class="time">${elapsed(s.sinceMs)}</div>
        </div>`).join("")
    : `<div class="empty">No active sessions</div>`;

  card.innerHTML = `<div class="head"><span class="title">CLAUDE</span><span class="counts">${counts}</span></div>${rows}`;

  const height = windowHeight(sessions.length, compact);
  if (height !== lastHeight) {
    lastHeight = height;
    invoke("fit", { height });
  }
}

document.addEventListener("mousedown", (e) => {
  if (e.button === 0) getCurrentWindow().startDragging();
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
