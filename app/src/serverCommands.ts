import type { ServerInfo } from "./api";

export type Lifecycle = "start" | "stop" | "restart";
export type ServerCommand = Lifecycle | "backup";

export const serverKey = (device: string, server: string) => JSON.stringify([device, server]);

export function commandReason(server: ServerInfo, action: ServerCommand, backupPending: boolean) {
  if (server.external) return "External server";
  if (action === "backup") return backupPending ? "Backup in progress" : undefined;
  if (server.state === "starting" || server.state === "stopping") return "Server is changing state";
  const running = server.state === "running";
  if (running && action === "start") return "Already running";
  if (!running && action !== "start") return "Server is stopped";
}

export const isPregenActive = (state: string | undefined) => state === "running" || state === "paused";
