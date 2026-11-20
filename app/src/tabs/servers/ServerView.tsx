import { useState } from "react";
import { Check, Copy, Play, RotateCw, Square } from "lucide-react";
import { api, minecraftAddress, type Device, type ServerInfo } from "~/api";
import { Badge } from "~/components/ui/badge";
import { Button } from "~/components/ui/button";
import { errorText, usePoll } from "~/lib/hooks";
import { toast } from "~/lib/toast";
import { cn } from "~/lib/utils";
import { BackupsPanel } from "./BackupsPanel";
import { ConsolePanel } from "./ConsolePanel";
import { ModsPanel } from "./ModsPanel";
import { OverviewPanel } from "./OverviewPanel";
import { SettingsPanel } from "./SettingsPanel";
import { STATE_LABEL, StateDot } from "./StateDot";
import { WorldPanel } from "./WorldPanel";

const PANELS = ["Overview", "Console", "Settings", "Mods", "Backups", "World"] as const;
type Panel = (typeof PANELS)[number];

export function ServerView({ device, server }: { device: Device; server: ServerInfo }) {
  const [panel, setPanel] = useState<Panel>("Overview");
  const [busy, setBusy] = useState<string | null>(null);
  const address = minecraftAddress(device, server);
  const addresses = usePoll(() => api.addresses(device.id, server.name), 15_000, [device.id, server.name]);
  const running = server.state === "running" || server.state === "starting";

  const act = async (action: "start" | "stop" | "restart") => {
    setBusy(action);
    try {
      await api.action(device.id, server.name, action);
    } catch (err) {
      toast(errorText(err), "error");
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-4 border-b px-6 pt-5 pb-4">
        <div className="flex min-w-0 flex-1 flex-col gap-1.5">
          <div className="flex items-center gap-2.5">
            <h1 className="truncate font-semibold text-xl tracking-tight">{server.name}</h1>
            <Badge variant="outline" className="gap-1.5">
              <StateDot state={server.state} /> {STATE_LABEL[server.state]}
            </Badge>
            {server.external && <Badge variant="secondary">Not started by moddedMC</Badge>}
          </div>
          <div className="flex flex-wrap items-center gap-2">
            {(addresses.value?.addresses ?? []).map((a) => (
              <AddressChip key={a.address} label={KIND_LABEL[a.kind]} address={a.address} />
            ))}
            <AddressChip label="This PC" address={address} />
            {addresses.value?.playit === "stopped" && (
              <Button
                size="xs"
                variant="outline"
                onClick={() => api.startPlayit(device.id, server.name).then(addresses.refresh)}
              >
                Start playit
              </Button>
            )}
            {addresses.value?.playit === "no-tunnel" && server.port && (
              <span className="text-muted-foreground text-xs">
                No playit tunnel points at port {server.port}. Add one at playit.gg to get a public address.
              </span>
            )}
          </div>
        </div>
        {!server.external && (
          <div className="flex gap-2">
            {running ? (
              <>
                <Button variant="outline" disabled={!!busy} onClick={() => act("restart")}>
                  <RotateCw className={cn(busy === "restart" && "animate-spin")} /> Restart
                </Button>
                <Button variant="destructive-outline" disabled={!!busy} onClick={() => act("stop")}>
                  <Square /> Stop
                </Button>
              </>
            ) : (
              <Button disabled={!!busy || server.state === "stopping"} onClick={() => act("start")}>
                <Play /> Start
              </Button>
            )}
          </div>
        )}
      </header>
      <nav className="flex gap-1 border-b px-4">
        {PANELS.map((p) => (
          <button
            key={p}
            onClick={() => setPanel(p)}
            className={cn(
              "-mb-px border-b-2 px-3 py-2 font-medium text-sm transition-colors",
              panel === p
                ? "border-primary text-foreground"
                : "border-transparent text-muted-foreground hover:text-foreground",
            )}
          >
            {p}
          </button>
        ))}
      </nav>
      <div key={panel} className="min-h-0 flex-1 animate-fade overflow-y-auto">
        {panel === "Overview" && <OverviewPanel device={device} server={server} />}
        {panel === "Console" && <ConsolePanel device={device} server={server} />}
        {panel === "Settings" && <SettingsPanel device={device} server={server} />}
        {panel === "Mods" && <ModsPanel device={device} server={server} />}
        {panel === "Backups" && <BackupsPanel device={device} server={server} />}
        {panel === "World" && <WorldPanel device={device} server={server} />}
      </div>
    </div>
  );
}

const KIND_LABEL = { playit: "playit.gg", tailscale: "Tailscale", lan: "LAN" } as const;

/** One address friends can join with; click to copy. */
function AddressChip({ label, address }: { label: string; address: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      onClick={async () => {
        await navigator.clipboard.writeText(address);
        setCopied(true);
        window.setTimeout(() => setCopied(false), 1500);
      }}
      className="flex items-center gap-1.5 rounded-md border px-2 py-0.5 text-sm hover:bg-accent"
      title={`Copy the ${label} address`}
    >
      <span className="text-muted-foreground text-xs">{label}</span>
      <span className="font-mono">{address}</span>
      {copied ? (
        <Check className="size-3.5 text-success-foreground" />
      ) : (
        <Copy className="size-3.5 text-muted-foreground" />
      )}
    </button>
  );
}
