"use strict";

const { contextBridge, ipcRenderer } = require("electron");

contextBridge.exposeInMainWorld("kianaDesktop", {
  state: () => ipcRenderer.invoke("workspace:state"),
  openFolder: () => ipcRenderer.invoke("workspace:open"),
  newProject: () => ipcRenderer.invoke("workspace:new"),
  continueLast: () => ipcRenderer.invoke("workspace:continue"),
});
