import { contextBridge } from "electron";

contextBridge.exposeInMainWorld("synthDesktop", {
	platform: process.platform
});
