import assert from "node:assert/strict";
import { test } from "node:test";
import {
  agentLines,
  contextLevel,
  elapsed,
  escapeHtml,
  folderName,
  infoTitle,
  label,
  layout,
  windowHeight,
} from "../src/format.js";

const NOW = 1_800_000_000_000;

test("elapsed formats seconds, minutes, hours and days", () => {
  assert.equal(elapsed(NOW - 5_000, NOW), "5s");
  assert.equal(elapsed(NOW - 125_000, NOW), "2m");
  assert.equal(elapsed(NOW - (3 * 3600 + 7 * 60) * 1000, NOW), "3h07");
  assert.equal(elapsed(NOW - 2 * 86_400_000, NOW), "2d");
});

test("elapsed never goes negative when the clock is slightly behind", () => {
  assert.equal(elapsed(NOW + 3_000, NOW), "0s");
});

test("label maps known kinds and falls back to the raw status", () => {
  assert.equal(label("busy", ""), "Busy");
  assert.equal(label("idle", ""), "Idle");
  assert.equal(label("attention", ""), "Needs you");
  assert.equal(label("unknown", "weird"), "weird");
  assert.equal(label("unknown", ""), "Unknown");
});

test("folderName handles both separators and trailing slashes", () => {
  assert.equal(folderName("C:\\work\\my-repo\\"), "my-repo");
  assert.equal(folderName("/home/me/proj/"), "proj");
});

const sessions = (n, agents = []) => Array.from({ length: n }, () => ({ agents }));
const task = (description, extra = {}) => ({ description, name: "", kind: "local_agent", agentType: "", startTime: 1, ...extra });

test("windowHeight keeps room for an empty state and shrinks in compact mode", () => {
  assert.equal(windowHeight([], false), 34 + 46 + 8);
  assert.equal(windowHeight(sessions(3), false), 34 + 3 * 46 + 8);
  assert.equal(windowHeight(sessions(3), true), 34 + 3 * 28 + 8);
});

test("windowHeight grows with running agents, but not in compact mode", () => {
  const two = sessions(2, [task("a"), task("b")]);
  assert.equal(windowHeight(two, false), 34 + 2 * (46 + 2 * 16) + 8);
  assert.equal(windowHeight(two, true), 34 + 2 * 28 + 8);
});

test("agentLines names the agent type and folds the overflow into one line", () => {
  assert.deepEqual(agentLines([task("Find auth", { agentType: "Explore" })]), [{ text: "Explore: Find auth", sinceMs: 1 }]);
  assert.equal(agentLines([task(""), task("", { name: "scout" }), task("", { kind: "local_bash" })]).map((l) => l.text).join("|"), "local_agent|scout|local_bash");

  const many = agentLines([task("a"), task("b"), task("c"), task("d"), task("e")]);
  assert.deepEqual(many.map((l) => l.text), ["a", "b", "+3 more"]);
  assert.equal(many.length, 3);
});

test("windowHeight adds room for extra lines", () => {
  assert.equal(windowHeight(sessions(1), false, 15), 34 + 46 + 15 + 8);
});

const HOUR = 3_600_000;
const withState = (list, extra = {}) => ({ sessions: list, compact: false, foldIdle: false, autoCollapse: false, notice: null, ...extra });
const live = (kind, ago = 1000) => ({ kind, sinceMs: NOW - ago, agents: [] });

test("layout folds sessions idle for over an hour when asked", () => {
  const list = [live("busy"), live("idle", 2 * HOUR), live("idle", 5 * 60_000)];
  assert.equal(layout(withState(list), false, NOW).shown.length, 3);

  const folded = layout(withState(list, { foldIdle: true }), false, NOW);
  assert.equal(folded.shown.length, 2);
  assert.equal(folded.folded.length, 1);
  assert.equal(folded.height, 34 + 2 * 46 + 28 + 8);
});

test("layout makes room for a notice", () => {
  assert.equal(layout(withState([live("busy")], { notice: "hi" }), false, NOW).height, 34 + 46 + 15 + 8);
});

test("layout leaves no empty-state row when everything is folded", () => {
  const result = layout(withState([live("idle", 2 * HOUR)], { foldIdle: true }), false, NOW);
  assert.equal(result.height, 34 + 28 + 8);
});

test("layout collapses to the header only while nothing runs and the pointer is away", () => {
  const quiet = withState([live("idle"), live("idle")], { autoCollapse: true });
  assert.equal(layout(quiet, false, NOW).collapsed, true);
  assert.equal(layout(quiet, false, NOW).height, 34 + 8);
  assert.equal(layout(quiet, true, NOW).collapsed, false);
  assert.equal(layout(withState([live("busy")], { autoCollapse: true }), false, NOW).collapsed, false);
  assert.equal(layout(withState([live("attention")], { autoCollapse: true }), false, NOW).collapsed, false);
  assert.equal(layout(withState([live("idle")]), false, NOW).collapsed, false);
});

test("contextLevel and infoTitle describe a session's context and cost", () => {
  assert.equal(contextLevel(50), "");
  assert.equal(contextLevel(80), "warning");
  assert.equal(contextLevel(97), "critical");
  assert.equal(infoTitle({ model: "Opus", contextPercent: 42.4, costUsd: 1.234 }), "Opus · context 42 % · $1.23");
  assert.equal(infoTitle({ model: "", contextPercent: null, costUsd: 0 }), "");
  assert.equal(infoTitle(null), "");
});

test("escapeHtml neutralises markup in session names", () => {
  assert.equal(escapeHtml(`<img src=x onerror="a">&'`), "&lt;img src=x onerror=&quot;a&quot;&gt;&amp;&#39;");
});
