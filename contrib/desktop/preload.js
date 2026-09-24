"use strict";

const { contextBridge, ipcRenderer } = require("electron");

// Keep the sandboxed preload self-contained. The main process is the authority
// for these values; the preload only carries the short-lived handshake envelope
// and never receives the Web bearer credential.
const HANDSHAKE_CHANNEL = "desktop:handshake";
const IPC_PROTOCOL_VERSION = "kiana.desktop.ipc.v1";
const IPC_CHANNELS = Object.freeze({
  state: "workspace:state",
  open: "workspace:open",
  newProject: "workspace:new",
  continue: "workspace:continue",
});

let bridgeSession = null;
let requestChain = Promise.resolve();

async function establishSession() {
  if (bridgeSession) {
    return bridgeSession;
  }
  const response = await ipcRenderer.invoke(HANDSHAKE_CHANNEL, {
    version: IPC_PROTOCOL_VERSION,
  });
  if (
    !response ||
    response.version !== IPC_PROTOCOL_VERSION ||
    typeof response.instance_token !== "string" ||
    !Number.isSafeInteger(response.nonce) ||
    !(response.workspace_binding === null || typeof response.workspace_binding === "string")
  ) {
    throw new Error("desktop_ipc_handshake_invalid");
  }
  bridgeSession = {
    instanceToken: response.instance_token,
    nonce: response.nonce,
    workspaceBinding: response.workspace_binding,
  };
  return bridgeSession;
}

function invokeIntent(channel) {
  // Electron preserves invoke ordering, but serializing here makes the nonce
  // monotonic even when two UI buttons are clicked in the same task turn.
  requestChain = requestChain.then(async () => {
    const session = await establishSession();
    const nonce = session.nonce + 1;
    const envelope = {
      version: IPC_PROTOCOL_VERSION,
      instance_token: session.instanceToken,
      nonce,
      workspace_binding: session.workspaceBinding,
    };
    const result = await ipcRenderer.invoke(channel, envelope);
    session.nonce = nonce;
    return result;
  });
  return requestChain;
}

contextBridge.exposeInMainWorld("kianaDesktop", {
  state: () => invokeIntent(IPC_CHANNELS.state),
  openFolder: () => invokeIntent(IPC_CHANNELS.open),
  newProject: () => invokeIntent(IPC_CHANNELS.newProject),
  continueLast: () => invokeIntent(IPC_CHANNELS.continue),
});
