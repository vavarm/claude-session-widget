import assert from "node:assert/strict";
import { test } from "node:test";
import {
  agentLines,
  elapsed,
  escapeHtml,
  extraHeight,
  folderName,
  label,
  quotaView,
  untilReset,
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

test("untilReset formats minutes, hours and days", () => {
  assert.equal(untilReset(1000 + 30, 1000), "1m");
  assert.equal(untilReset(1000 + 25 * 60, 1000), "25m");
  assert.equal(untilReset(1000 + (2 * 3600 + 5 * 60), 1000), "2h05");
  assert.equal(untilReset(1000 + (50 * 3600), 1000), "2d02h");
  assert.equal(untilReset(900, 1000), "1m");
});

test("quotaView shows a reset note only for windows at the limit", () => {
  const now = 1000;
  const view = quotaView({
    fiveHour: { usedPercentage: 95.4, resetsAt: now + 3600 },
    sevenDay: { usedPercentage: 41.2, resetsAt: now + 86_400 },
  }, now);
  assert.deepEqual(view.bars, [
    { name: "5h", percent: 95, limit: true },
    { name: "7d", percent: 41, limit: false },
  ]);
  assert.deepEqual(view.notes, ["5h limit: resets in 1h00"]);
});

test("quotaView copes with missing windows and absent quotas", () => {
  assert.deepEqual(quotaView(null), { bars: [], notes: [] });
  assert.equal(quotaView({ sevenDay: { usedPercentage: 150, resetsAt: 5000 } }, 1000).bars[0].percent, 100);
});

test("extraHeight adds the bars once and one line per note", () => {
  const view = { bars: [{}, {}], notes: ["a"] };
  assert.equal(extraHeight(view, null), 26 + 15);
  assert.equal(extraHeight(view, "notice"), 26 + 30);
  assert.equal(extraHeight({ bars: [], notes: [] }, "notice"), 15);
  assert.equal(windowHeight(sessions(1), false, 26), 34 + 46 + 26 + 8);
});

test("escapeHtml neutralises markup in session names", () => {
  assert.equal(escapeHtml(`<img src=x onerror="a">&'`), "&lt;img src=x onerror=&quot;a&quot;&gt;&amp;&#39;");
});
