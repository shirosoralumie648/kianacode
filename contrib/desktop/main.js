"use strict";

const {
  app,
  BrowserWindow,
  Menu,
  Notification,
  Tray,
  dialog,
  ipcMain,
  nativeImage,
  net,
  shell,
} = require("electron");
const { spawn } = require("child_process");
const fs = require("fs");
const os = require("os");
const path = require("path");
const { pathToFileURL } = require("url");
const { waitForReady, readInstanceSidecar, probeReady } = require("./lib/readiness");
const { findKiana, kianaArgs } = require("./lib/find-kiana");
const { BUTTONS, closeDecision, closePrompt } = require("./lib/close-policy");
const {
  closeAttention,
  createDesktopState,
  reduceDesktopState,
} = require("./lib/desktop-state");
const {
  DesktopNotificationAdapter,
  NotificationBridge,
} = require("./lib/notifications");
const {
  emptyDesktopStore,
  mergeDesktopStore,
  readDesktopStore,
  reattachPlan,
  validateServerReference,
  writeDesktopStore,
} = require("./lib/desktop-persistence");
const { groupExists, stopWorker } = require("./lib/worker");
const {
  createScratchWorkspace,
  isScratchWorkspace,
} = require("./lib/workspace");
const {
  CHANNEL_INTENTS,
  HANDSHAKE_CHANNEL,
  classifyNavigation,
  createIpcSession,
  opaqueToken,
  validateHandshake,
  validateIpcRequest,
} = require("./lib/ipc-security");

let mainWindow = null;
let tray = null;
let child = null;
let quitting = false;
let workdir = null;
let lastError = "";
let stopPromise = null;
let quitPromise = null;
let shutdownComplete = false;
let workspaceTransition = Promise.resolve();
let ipcSession = null;
let lifecycle = "stopped";
let readyRecord = null;
let desktopState = createDesktopState();
const notificationBridge = new NotificationBridge({ workspaceBindingDigest: null });
const notificationAdapter = new DesktopNotificationAdapter({
  bridge: notificationBridge,
  notifier: Notification,
  permission: "unknown",
});

function welcomeUrl() {
  return pathToFileURL(path.join(__dirname, "welcome.html")).href;
}

function rotateIpcSession({ origin, workspaceBinding = null }) {
  ipcSession = createIpcSession({ origin, workspaceBinding });
  notificationAdapter.setWorkspaceBindingDigest(ipcSession.workspaceBindingDigest);
}

rotateIpcSession({ origin: welcomeUrl() });

function applyDesktopState(event) {
  desktopState = reduceDesktopState(desktopState, event);
}

function configPath() {
  return path.join(app.getPath("userData"), "desktop.json");
}

function loadConfig() {
  try {
    return readDesktopStore(configPath());
  } catch {
    return emptyDesktopStore();
  }
}

function saveConfig(patch) {
  const next = mergeDesktopStore(loadConfig(), patch);
  return writeDesktopStore(configPath(), next);
}

function iconImage() {
  const png = path.join(__dirname, "icon.png");
  if (fs.existsSync(png)) {
    return nativeImage.createFromPath(png);
  }
  return nativeImage.createEmpty();
}

function resolveKiana() {
  const extra = [];
  if (process.resourcesPath) {
    extra.push(path.join(process.resourcesPath, "kiana"));
    extra.push(path.join(process.resourcesPath, "kiana.exe"));
  }
  extra.push(path.join(__dirname, "..", "..", "target", "debug", "kiana"));
  extra.push(path.join(__dirname, "..", "..", "target", "release", "kiana"));
  const found = findKiana({ extraCandidates: extra });
  if (!found) {
    throw new Error(
      "kiana binary not found. Rebuild current tree and reinstall the .deb."
    );
  }
  return found;
}

async function stopChild() {
  if (stopPromise) return stopPromise;
  const proc = child;
  if (!proc) return;
  lifecycle = "stopping";
  applyDesktopState({ type: "worker_stopping" });
  proc.kianaStopping = true;
  stopPromise = stopWorker(proc).then(() => {
    const instanceRef = readyRecord ? {
      instance_id: readyRecord.instance_id,
      authority_epoch: readyRecord.authority_epoch,
      feed_epoch: readyRecord.epoch,
    } : undefined;
    const detachPatch = { detached: true };
    if (instanceRef) detachPatch.instance_ref = instanceRef;
    saveConfig(detachPatch);
    if (child === proc) child = null;
    readyRecord = null;
    lifecycle = "stopped";
    applyDesktopState({ type: "worker_stopped" });
  }).catch(error => {
    lifecycle = "unconfirmed";
    applyDesktopState({ type: "worker_failed", unconfirmed: true });
    throw error;
  }).finally(() => { stopPromise = null; });
  return stopPromise;
}

function quitGracefully() {
  if (quitPromise) return quitPromise;
  quitting = true;
  quitPromise = stopChild().then(() => {
    shutdownComplete = true;
    app.quit();
  }).catch(error => {
    quitting = false; quitPromise = null;
    lastError = String(error.message || error);
    if (mainWindow && !mainWindow.isDestroyed()) mainWindow.show();
    dialog.showErrorBox("无法确认工作进程已停止", lastError + "\n请检查任务回执后重试退出。");
  });
  return quitPromise;
}

async function autoTrust(url) {
  try {
    await net.fetch(url.replace(/\/$/, "") + "/api/trust", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: "{}",
    });
  } catch {
    /* Trust stays a button in the workbench if this fails. */
  }
}

async function startHarness(folder, { autoTrustScratch = false } = {}) {
  if (quitting) throw new Error("desktop_shutting_down");
  await stopChild();
  if (quitting) throw new Error("desktop_shutting_down");
  lifecycle = "starting";
  const kiana = resolveKiana();
  const workspace = fs.realpathSync(folder);
  const readyNonce = opaqueToken("ready");
  child = spawn(kiana, kianaArgs(workspace), {
    stdio: ["ignore", "pipe", "pipe"],
    env: { ...process.env, KIANA_DESKTOP_READY_NONCE: readyNonce },
    cwd: workspace,
    detached: process.platform !== "win32",
    windowsHide: true,
  });
  child.stdout.setEncoding("utf8");
  child.stderr.setEncoding("utf8");
  // Drain diagnostics during the structured ready handshake, but never interpret stderr.
  child.stderr.resume();
  const proc = child;
  applyDesktopState({ type: "workspace_requested", workspace });
  proc.on("exit", code => {
    if (child !== proc || proc.kianaStopping || quitting) return;
    readyRecord = null;
    if (process.platform !== "win32" && groupExists(proc.pid)) {
      lifecycle = "unconfirmed";
      applyDesktopState({ type: "worker_failed", unconfirmed: true });
      lastError = `kiana web stopped (${code ?? "?"}); worker process group exit is unconfirmed.`;
    } else {
      child = null;
      lifecycle = "failed";
      applyDesktopState({ type: "worker_failed", unconfirmed: false });
      lastError = `kiana web stopped (${code ?? "?"}).`;
    }
    showWelcome();
  });
  let ready;
  try {
    ready = await waitForReady(proc, { nonce: readyNonce, workspace, pid: proc.pid });
    const verifyInstance = async () => {
      if (proc.exitCode !== null || proc.signalCode !== null) {
        throw new Error("desktop_worker_not_live");
      }
      readInstanceSidecar(workspace, ready, proc.pid);
    };
    await verifyInstance();
    await probeReady(ready, (url, options) => net.fetch(url, options), { verifyInstance });
    await verifyInstance();
  }
  catch (error) {
    const original = error;
    try { await stopChild(); } catch (stopError) { throw stopError; }
    lifecycle = "failed";
    throw original;
  }
  workdir = workspace;
  // Rotate the desktop bridge with the worker instance. The opaque binding
  // never contains the Web bearer credential or a workspace path.
  rotateIpcSession({
    origin: new URL(ready.url).origin,
    workspaceBinding: opaqueToken("workspace"),
  });
  lifecycle = "ready";
  readyRecord = ready;
  applyDesktopState({
    type: "worker_ready",
    instance_id: ready.instance_id,
    epoch: ready.epoch,
  });
  saveConfig({
    workspace_path: workspace,
    instance_ref: {
      instance_id: ready.instance_id,
      authority_epoch: ready.authority_epoch,
      feed_epoch: ready.epoch,
    },
    last_cursor: { epoch: ready.epoch, sequence: 0 },
    detached: false,
  });
  // The stderr diagnostics pipe is already drained; resume the stdout display stream after ready.
  proc.stdout.resume();
  if (autoTrustScratch && isScratchWorkspace(workspace)) {
    await autoTrust(ready.url);
  }
  return ready.url;
}

function showWelcome() {
  if (!mainWindow || mainWindow.isDestroyed()) {
    return;
  }
  rotateIpcSession({ origin: welcomeUrl() });
  mainWindow.loadFile(path.join(__dirname, "welcome.html"));
}

async function switchWorkspace(folder, options = {}) {
  lastError = "";
  // The old renderer loses its IPC binding before its worker is stopped.
  showWelcome();
  try {
    const url = await startHarness(folder, options);
    if (mainWindow && !mainWindow.isDestroyed()) {
      await mainWindow.loadURL(url);
      mainWindow.show();
    }
    return { ok: true, folder: workdir, url };
  } catch (error) {
    lastError = String(error.message || error);
    if (lifecycle !== "unconfirmed") lifecycle = "failed";
    if (lifecycle === "failed") applyDesktopState({ type: "worker_failed", unconfirmed: false });
    showWelcome();
    throw error;
  }
}

function openWorkspace(folder, options = {}) {
  workspaceTransition = workspaceTransition.catch(() => {}).then(() => switchWorkspace(folder, options));
  return workspaceTransition;
}

async function pickFolder() {
  const result = await dialog.showOpenDialog(mainWindow || undefined, {
    title: "打开文件夹",
    defaultPath: workdir || os.homedir(),
    properties: ["openDirectory", "createDirectory"],
    buttonLabel: "Open",
  });
  if (result.canceled || !result.filePaths[0]) {
    return null;
  }
  return result.filePaths[0];
}

function createWindow() {
  mainWindow = new BrowserWindow({
    width: 1280,
    height: 800,
    title: "Kiana",
    icon: iconImage(),
    webPreferences: {
      preload: path.join(__dirname, "preload.js"),
      sandbox: true,
      contextIsolation: true,
      nodeIntegration: false,
      webSecurity: true,
      allowRunningInsecureContent: false,
      webviewTag: false,
    },
  });
  installWindowSecurity(mainWindow);
  mainWindow.on("close", (event) => {
    if (quitting) {
      return;
    }
    event.preventDefault();
    const choice = dialog.showMessageBoxSync(mainWindow, {
      type: "question",
      buttons: BUTTONS,
      defaultId: 0,
      cancelId: 2,
      title: "Kiana",
      message: "Close the window?",
      detail: closePrompt(closeAttention(desktopState)).detail,
    });
    const decision = closeDecision(choice);
    if (decision === "keep") {
      mainWindow.hide();
    } else if (decision === "quit") {
      void quitGracefully();
    }
  });
  showWelcome();
}

function createMenu() {
  Menu.setApplicationMenu(
    Menu.buildFromTemplate([
      {
        label: "File",
        submenu: [
          {
            label: "Open folder…",
            accelerator: "CmdOrCtrl+O",
            click: () => {
              void handleOpenFolder();
            },
          },
          {
            label: "New project in ~/.kiana",
            accelerator: "CmdOrCtrl+N",
            click: () => {
              void handleNewProject();
            },
          },
          { type: "separator" },
          { role: "quit" },
        ],
      },
      { role: "editMenu" },
      { role: "viewMenu" },
    ])
  );
}

function createTray() {
  tray = new Tray(iconImage());
  tray.setToolTip("Kiana");
  tray.setContextMenu(
    Menu.buildFromTemplate([
      {
        label: "Show window",
        click: () => {
          if (mainWindow) {
            mainWindow.show();
            mainWindow.focus();
          }
        },
      },
      {
        label: "Open folder…",
        click: () => {
          void handleOpenFolder();
        },
      },
      {
        label: "New project in ~/.kiana",
        click: () => {
          void handleNewProject();
        },
      },
      { type: "separator" },
      {
        label: "Quit",
        click: () => {
          void quitGracefully();
        },
      },
    ])
  );
  tray.on("click", () => {
    if (mainWindow) {
      mainWindow.show();
      mainWindow.focus();
    }
  });
}

async function handleOpenFolder() {
  const folder = await pickFolder();
  if (!folder) {
    return { canceled: true };
  }
  return openWorkspace(folder, { autoTrustScratch: false });
}

async function handleNewProject() {
  const folder = createScratchWorkspace({ homedir: os.homedir() });
  return openWorkspace(folder, { autoTrustScratch: true });
}

async function handleContinue() {
  const plan = reattachPlan(loadConfig());
  if (!plan.workspace_path || !fs.existsSync(plan.workspace_path)) {
    throw new Error("没有上次的工作区。打开文件夹，或新建 ~/.kiana 项目。");
  }
  // Reattach is an explicit new worker handshake.  The persisted cursor is only a
  // stale reference for diagnostics; it is never submitted as a resume or action.
  return openWorkspace(plan.workspace_path, { autoTrustScratch: false });
}

function workspaceState() {
  const stored = loadConfig();
  const last = stored.workspace_ref && stored.workspace_ref.path;
  const attention = closeAttention(desktopState);
  return {
    last: last && fs.existsSync(last) ? last : null,
    current: workdir,
    lifecycle,
    instance: lifecycle === "ready" && readyRecord ? {
      id: readyRecord.instance_id,
      epoch: readyRecord.epoch,
    } : null,
    error: lastError,
    scratchRoot: path.join(os.homedir(), ".kiana", "workspaces"),
    workspace_binding_digest: ipcSession && ipcSession.workspaceBindingDigest,
    attention,
    persistence: {
      schema: stored.schema,
      detached: stored.detached,
      has_instance_ref: stored.instance_ref !== null,
      has_session_ref: stored.session_ref !== null,
      has_last_cursor: stored.last_cursor !== null,
      draft_policy: stored.draft_policy,
    },
  };
}

function rememberServerReference(reference) {
  try {
    const normalized = validateServerReference(
      reference,
      ipcSession && ipcSession.workspaceBindingDigest
    );
    saveConfig({ server_reference: normalized });
    return { ok: true, disposition: "stored" };
  } catch (error) {
    throw ipcDenied(error.message || "desktop_reference_invalid");
  }
}

function ipcDenied(reason) {
  const error = new Error(`desktop_ipc_denied:${reason}`);
  error.code = "desktop_ipc_denied";
  return error;
}

function resolveNotificationPermission() {
  if (typeof Notification !== "function" ||
      (typeof Notification.isSupported === "function" && !Notification.isSupported())) {
    return "unavailable";
  }
  if (Notification.permission === "denied") return "denied";
  if (Notification.permission === "granted") return "granted";
  return "unknown";
}

function notifyServerFact(fact) {
  notificationAdapter.setPermission(resolveNotificationPermission());
  const result = notificationAdapter.accept(fact);
  if (!result.ok) throw ipcDenied(result.reason);
  applyDesktopState({ type: "server_fact", fact });
  if (result.disposition !== "new") {
    return { ok: true, disposition: result.disposition };
  }
  return { ok: true, disposition: result.disposition };
}

function installWindowSecurity(window) {
  const expectedSender = window.webContents;
  window.webContents.on("will-navigate", (event, url) => {
    if (!ipcSession) {
      event.preventDefault();
      return;
    }
    const frame = event.senderFrame;
    if (!event.sender || event.sender !== expectedSender || !frame || frame.parent) {
      // The event's senderFrame is the only renderer identity accepted by the
      // shell. A missing/foreign frame cannot navigate the bound window.
      event.preventDefault();
      return;
    }
    const decision = classifyNavigation(url, {
      trustedOrigin: ipcSession.origin,
      welcomeUrl: welcomeUrl(),
    });
    if (decision.action !== "allow-loopback") {
      event.preventDefault();
    }
  });

  window.webContents.setWindowOpenHandler(details => {
    const decision = classifyNavigation(details.url, {
      trustedOrigin: ipcSession && ipcSession.origin,
      welcomeUrl: welcomeUrl(),
    });
    if (decision.action === "external") {
      // External navigation is explicit and leaves the renderer. The URL is
      // never put in an IPC response, query token or desktop log.
      void shell.openExternal(decision.url).catch(() => {});
      return { action: "deny" };
    }
    if (decision.action === "allow-loopback") {
      // Keep a same-origin popup constrained to the same Chromium security
      // posture. It has no preload bridge, so it cannot address workspace IPC.
      return {
        action: "allow",
        overrideBrowserWindowOptions: {
          webPreferences: {
            sandbox: true,
            contextIsolation: true,
            nodeIntegration: false,
            webSecurity: true,
            allowRunningInsecureContent: false,
            webviewTag: false,
          },
        },
      };
    }
    return { action: "deny" };
  });
}

function dispatchDesktopIntent(intent) {
  // This is the existing desktop adapter boundary. It accepts only the typed
  // intent emitted by the ControlPlane-facing validator; it never interprets
  // renderer text as a command and never starts a model/agent loop.
  switch (intent.name) {
    case "desktop.workspace.state.v1":
      return workspaceState();
    case "desktop.workspace.open.v1":
      return handleOpenFolder();
    case "desktop.workspace.new.v1":
      return handleNewProject();
    case "desktop.workspace.continue.v1":
      return handleContinue();
    case "desktop.notification.server-fact.v1":
      return notifyServerFact(intent.fact);
    case "desktop.session.reference.v1":
      return rememberServerReference(intent.reference);
    default:
      throw ipcDenied("unknown_intent");
  }
}

function registerIpcHandlers() {
  ipcMain.handle(HANDSHAKE_CHANNEL, (event, payload) => {
    const result = validateHandshake({
      event,
      expectedSender: mainWindow && mainWindow.webContents,
      session: ipcSession,
      welcomeUrl: welcomeUrl(),
      payload,
    });
    if (!result.ok) {
      throw ipcDenied(result.reason);
    }
    return result.value;
  });

  for (const [channel, intentName] of Object.entries(CHANNEL_INTENTS)) {
    ipcMain.handle(channel, async (event, envelope) => {
      const result = validateIpcRequest({
        event,
        expectedSender: mainWindow && mainWindow.webContents,
        session: ipcSession,
        welcomeUrl: welcomeUrl(),
        channel,
        envelope,
      });
      if (!result.ok) {
        throw ipcDenied(result.reason);
      }
      // `result.intent` is the only renderer-originated value admitted to the
      // desktop action boundary. It is versioned and carries no URL, token,
      // path or arbitrary renderer arguments.
      if (result.intent.name !== intentName) {
        throw ipcDenied("intent_contract_mismatch");
      }
      return dispatchDesktopIntent(result.intent);
    });
  }
}

registerIpcHandlers();

app.whenReady().then(() => {
  createWindow();
  createMenu();
  createTray();
});

app.on("window-all-closed", () => {
  if (quitting) {
    app.quit();
  }
});

app.on("before-quit", event => {
  if (shutdownComplete) return;
  event.preventDefault();
  void quitGracefully();
});
