import assert from "node:assert/strict";
import { test } from "node:test";
import { elapsed, escapeHtml, folderName, label, windowHeight } from "../src/format.js";

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

test("windowHeight keeps room for an empty state and shrinks in compact mode", () => {
  assert.equal(windowHeight(0, false), 34 + 46 + 8);
  assert.equal(windowHeight(3, false), 34 + 3 * 46 + 8);
  assert.equal(windowHeight(3, true), 34 + 3 * 28 + 8);
});

test("escapeHtml neutralises markup in session names", () => {
  assert.equal(escapeHtml(`<img src=x onerror="a">&'`), "&lt;img src=x onerror=&quot;a&quot;&gt;&amp;&#39;");
});
