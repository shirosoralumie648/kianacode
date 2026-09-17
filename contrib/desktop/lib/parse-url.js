"use strict";

function parseWebUrl(text) {
  const source = String(text || "");
  const tagged = source.match(/KIANA_WEB_URL=(http:\/\/127\.0\.0\.1:\d+)/);
  if (tagged) {
    return tagged[1];
  }
  const bare = source.match(/http:\/\/127\.0\.0\.1:\d+/);
  return bare ? bare[0] : null;
}

function isLoopbackBind(value) {
  const trimmed = String(value || "").trim();
  return (
    trimmed.startsWith("127.0.0.1:") ||
    trimmed.startsWith("localhost:") ||
    trimmed.startsWith("[::1]:") ||
    trimmed.startsWith("::1:")
  );
}

module.exports = { parseWebUrl, isLoopbackBind };
