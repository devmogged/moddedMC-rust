import { useState } from "react";
import { AlertCircle, Check, Copy, Play, RotateCw, Square } from "lucide-react";
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

export function ServerView({
  device,
  server,
  onChanged,
  onPortSaved,
  onDeleted,
}: {
  device: Device;
  server: ServerInfo;
  onChanged: () => void;
  onPortSaved: (port: number) => void;
  onDeleted: () => void;
}) {
  const [panel, setPanel] = useState<Panel>("Overview");
  const [busy, setBusy] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const address = minecraftAddress(device, server);
  const addresses = usePoll(() => api.addresses(device.id, server.name), 15_000, [device.id, server.name]);
  const running = server.state === "running" || server.state === "starting";
  const transitioning = server.state === "starting" || server.state === "stopping";
  const startError = actionError ?? (busy ? null : server.start_error);

  const act = async (action: "start" | "stop" | "restart") => {
    setBusy(action);
    setActionError(null);
    try {
      await api.action(device.id, server.name, action);
    } catch (err) {
      if (action === "stop") toast(errorText(err), "error");
      else setActionError(errorText(err));
    } finally {
      onChanged();
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
            <AddressChip label={device.route === "local" ? "This PC" : device.name} address={address} />
            {addresses.value &&
              addresses.value.playit !== "running" &&
              addresses.value.playit !== "unknown" && (
                <PlayitButton
                  device={device}
                  server={server}
                  state={addresses.value.playit}
                  onDone={addresses.refresh}
                />
              )}
          </div>
        </div>
        {!server.external && (
          <div className="flex gap-2">
            {running ? (
              <>
                <Button variant="outline" disabled={!!busy || transitioning} onClick={() => act("restart")}>
                  <RotateCw className={cn(busy === "restart" && "animate-spin")} /> Restart
                </Button>
                <Button
                  variant="destructive-outline"
                  disabled={!!busy || transitioning}
                  onClick={() => act("stop")}
                >
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
      {startError && (
        <div
          role="alert"
          className="flex items-center gap-2 border-b bg-destructive/8 px-6 py-2 text-destructive-foreground text-sm"
        >
          <AlertCircle className="size-4 shrink-0" />
          <span className="flex-1">{startError}</span>
          {panel !== "Settings" && (
            <Button size="sm" variant="outline" onClick={() => setPanel("Settings")}>
              Open settings
            </Button>
          )}
        </div>
      )}
      <nav className="flex gap-1 border-b px-4">
        {PANELS.filter((p) => p !== "Settings" || !server.external).map((p) => (
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
        {panel === "Settings" && !server.external && (
          <SettingsPanel
            device={device}
            server={server}
            lifecycleBusy={!!busy}
            onPortSaved={(port) => {
              setActionError(null);
              onPortSaved(port);
              addresses.refresh();
            }}
            onDeleted={onDeleted}
          />
        )}
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

/**
 * Gets this server a playit.gg address: signs in to playit if needed (the user approves moddedMC
 * in their browser), makes a tunnel to the server's port, and starts playit.
 */
function PlayitButton({
  device,
  server,
  state,
  onDone,
}: {
  device: Device;
  server: ServerInfo;
  state: "signed-out" | "reconnect" | "no-tunnel" | "stopped";
  onDone: () => void;
}) {
  const [step, setStep] = useState<string | null>(null);

  const signIn = async () => {
    const { url } = await api.playitClaim(device.id);
    if (!url) return;
    setStep("Approve moddedMC on playit.gg");
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
    for (let i = 0; i < 450; i++) {
      await new Promise((r) => setTimeout(r, 2000));
      const status = await api.playitStatus(device.id);
      if (status.signed_in) return;
      if (status.claim?.error) throw new Error(status.claim.error);
    }
    throw new Error("The playit sign-in timed out. Try again.");
  };

  const go = async () => {
    try {
      if (state === "reconnect") await api.playitSignOut(device.id);
      if (state === "signed-out" || state === "reconnect") await signIn();
      setStep("Setting up playit");
      try {
        await api.startPlayit(device.id, server.name);
      } catch (err) {
        if (!errorText(err).includes("Sign in with moddedMC")) throw err;
        await api.playitSignOut(device.id);
        await signIn();
        setStep("Setting up playit");
        await api.startPlayit(device.id, server.name);
      }
      setStep("Waiting for the playit address");
      for (let attempt = 0; attempt < 20; attempt++) {
        const setup = await api.addresses(device.id, server.name);
        if (setup.playit === "running" && setup.addresses.some((address) => address.kind === "playit")) {
          toast("Playit is ready. Share the playit.gg address with your friends.");
          return;
        }
        await new Promise((resolve) => setTimeout(resolve, 2000));
      }
      toast("Playit is still publishing the address. It will appear here when it is ready.");
    } catch (err) {
      toast(errorText(err), "error");
    } finally {
      setStep(null);
      onDone();
    }
  };

  return (
    <Button size="xs" variant="outline" disabled={!!step} onClick={go}>
      {step ??
        (state === "reconnect"
          ? "Reconnect playit"
          : state === "stopped"
            ? "Start playit"
            : "Get a playit address")}
    </Button>
  );
}
