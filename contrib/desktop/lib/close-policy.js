"use strict";

const BUTTONS = ["Keep in background", "Quit", "Cancel"];

function closePrompt(state = {}) {
  const pending = Number.isSafeInteger(state.pending_count) ? state.pending_count : 0;
  const unknown = Number.isSafeInteger(state.unknown_count) ? state.unknown_count : 0;
  const draft = state.draft_dirty === true;
  const attention = [];
  if (pending > 0) attention.push(`${pending} 个待处理服务端动作`);
  if (unknown > 0) attention.push(`${unknown} 个结果尚未确认`);
  if (draft) attention.push("有未保存的桌面草稿");
  const detail = attention.length > 0
    ? `${attention.join("；")}。保留后台不会取消运行；退出也不会自动恢复、信任或批准。`
    : "保留后台会继续运行 DaemonHost；退出会停止工作进程，但不会自动取消、恢复、信任或批准。";
  return Object.freeze({
    attention: attention.length > 0,
    pending_count: pending,
    unknown_count: unknown,
    draft_dirty: draft,
    detail,
    mutation: "none",
  });
}

function closeDecision(buttonIndex) {
  if (buttonIndex === 0) {
    return "keep";
  }
  if (buttonIndex === 1) {
    return "quit";
  }
  return "cancel";
}

module.exports = { BUTTONS, closeDecision, closePrompt };
