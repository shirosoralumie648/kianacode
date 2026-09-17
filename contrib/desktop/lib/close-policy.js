"use strict";

const BUTTONS = ["Keep in background", "Quit", "Cancel"];

function closeDecision(buttonIndex) {
  if (buttonIndex === 0) {
    return "keep";
  }
  if (buttonIndex === 1) {
    return "quit";
  }
  return "cancel";
}

module.exports = { BUTTONS, closeDecision };
