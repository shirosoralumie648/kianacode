"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const os = require("os");
const path = require("path");
const { parseWebUrl, isLoopbackBind } = require("../lib/parse-url");
const { closeDecision, BUTTONS } = require("../lib/close-policy");
const { findKiana, kianaArgs } = require("../lib/find-kiana");

test("parses machine-readable loopback URL from kiana web stdout", () => {
  const text = [
    "Kiana web",
    "folder: /tmp/project",
    "KIANA_WEB_URL=http://127.0.0.1:43123",
    "http://127.0.0.1:43123",
  ].join("\n");
  assert.equal(parseWebUrl(text), "http://127.0.0.1:43123");
});

test("rejects non-loopback URLs", () => {
  assert.equal(parseWebUrl("http://0.0.0.0:3080"), null);
  assert.equal(parseWebUrl("http://192.168.1.8:3080"), null);
  assert.equal(isLoopbackBind("0.0.0.0:3080"), false);
  assert.equal(isLoopbackBind("127.0.0.1:0"), true);
});

test("close dialog can keep the worker in the tray", () => {
  assert.deepEqual(BUTTONS, ["Keep in background", "Quit", "Cancel"]);
  assert.equal(closeDecision(0), "keep");
  assert.equal(closeDecision(1), "quit");
  assert.equal(closeDecision(2), "cancel");
});

test("desktop always starts kiana web on loopback with no browser open", () => {
  assert.deepEqual(kianaArgs("/tmp/project"), [
    "web",
    "--no-open",
    "--bind",
    "127.0.0.1:0",
    "--workdir",
    "/tmp/project",
  ]);
});

test("finds KIANA_BIN before homedir fallback", () => {
  const found = findKiana({
    env: { KIANA_BIN: "/opt/kiana" },
    homedir: os.homedir(),
    existsSync: (candidate) => candidate === "/opt/kiana",
  });
  assert.equal(found, "/opt/kiana");
});

test("falls back to ~/.local/bin/kiana", () => {
  const home = "/home/demo";
  const expected = path.join(home, ".local", "bin", "kiana");
  const found = findKiana({
    env: {},
    homedir: home,
    existsSync: (candidate) => candidate === expected,
  });
  assert.equal(found, expected);
});
