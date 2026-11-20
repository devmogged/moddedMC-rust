import { useEffect, useState } from "react";
import { FolderOpen, Globe, House, Network, Router } from "lucide-react";
import { api, type Device } from "~/api";
import { Button } from "~/components/ui/button";
import {
  Dialog,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogPanel,
  DialogPopup,
  DialogTitle,
} from "~/components/ui/dialog";
import { Input } from "~/components/ui/input";
import { Select, SelectItem, SelectPopup, SelectTrigger, SelectValue } from "~/components/ui/select";
import { errorText } from "~/lib/hooks";
import { toast } from "~/lib/toast";
import { cn } from "~/lib/utils";

const NAME = /^[A-Za-z0-9][A-Za-z0-9_-]{0,31}$/;

type Hosting = "lan" | "playit" | "tailscale" | "forward";

export const HOSTING: {
  id: Hosting;
  title: string;
  blurb: string;
  icon: typeof House;
  steps: (port: number) => string[];
}[] = [
  {
    id: "lan",
    title: "Same Wi-Fi",
    blurb: "Friends in your house. Nothing to set up.",
    icon: House,
    steps: (port) => [
      "Friends on your network open Multiplayer → Add Server.",
      `They use this PC's local address with port ${port} (shown on the server page, or run ipconfig and use the IPv4 address).`,
    ],
  },
  {
    id: "playit",
    title: "playit.gg",
    blurb: "Free, works anywhere, no router changes. Recommended.",
    icon: Globe,
    steps: (port) => [
      "Download the playit program from playit.gg/download and run it.",
      "Open the link it prints and sign in to claim this PC.",
      `Add a tunnel: Minecraft Java, local address 127.0.0.1, local port ${port}.`,
      "Share the address playit gives you (like something.joinmc.link). It works while playit is running.",
    ],
  },
  {
    id: "tailscale",
    title: "Tailscale",
    blurb: "Private: only people you invite can reach it.",
    icon: Network,
    steps: (port) => [
      "Install Tailscale from tailscale.com/download and sign in.",
      "Share this machine with your friends from the Tailscale admin console, and have them install Tailscale too.",
      `They join with this PC's Tailscale name or 100.x address and port ${port}.`,
    ],
  },
  {
    id: "forward",
    title: "Port forwarding",
    blurb: "For people comfortable with their router.",
    icon: Router,
    steps: (port) => [
      "Give this PC a fixed local address in your router (DHCP reservation).",
      `Forward TCP port ${port} to that address.`,
      "Allow the server through Windows Firewall when it asks.",
      'Share your public IP (search "what is my ip") with the port.',
    ],
  },
];

export function NewServerDialog({
  open,
  onOpenChange,
  devices,
  onCreated,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  devices: Device[];
  onCreated: (device: string, server: string) => void;
}) {
  const [step, setStep] = useState<1 | 2>(1);
  const [device, setDevice] = useState("local");
  const [name, setName] = useState("");
  const [port, setPort] = useState(25565);
  const [exe, setExe] = useState("");
  const [hosting, setHosting] = useState<Hosting>("playit");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (open) {
      setStep(1);
      setName("");
      setBusy(false);
      setDevice(devices.find((d) => d.route === "local")?.id ?? devices[0]?.id ?? "local");
    }
  }, [open, devices]);

  const valid = NAME.test(name) && port >= 1024 && port <= 65535 && exe.trim().length > 0;

  const browse = async () => {
    const { open: pick } = await import("@tauri-apps/plugin-dialog");
    const path = await pick({ multiple: false, filters: [{ name: "Server", extensions: ["exe", ""] }] });
    if (typeof path === "string") setExe(path);
  };

  const create = async () => {
    setBusy(true);
    try {
      await api.createServer(device, { name, exe: exe.trim(), port });
      onCreated(device, name);
      setStep(2);
    } catch (err) {
      toast(errorText(err), "error");
    } finally {
      setBusy(false);
    }
  };

  const guide = HOSTING.find((h) => h.id === hosting)!;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogPopup className="sm:max-w-xl">
        {step === 1 ? (
          <>
            <DialogHeader>
              <DialogTitle>New server</DialogTitle>
              <DialogDescription>
                A fresh world. You can change everything later in its Settings.
              </DialogDescription>
            </DialogHeader>
            <DialogPanel className="flex flex-col gap-4">
              {devices.length > 1 && (
                <Field label="Runs on">
                  <Select value={device} onValueChange={(v) => v && setDevice(v)}>
                    <SelectTrigger>
                      <SelectValue>{(v: string) => devices.find((d) => d.id === v)?.name ?? v}</SelectValue>
                    </SelectTrigger>
                    <SelectPopup>
                      {devices.map((d) => (
                        <SelectItem key={d.id} value={d.id}>
                          {d.name}
                        </SelectItem>
                      ))}
                    </SelectPopup>
                  </Select>
                </Field>
              )}
              <Field label="Name" hint="Letters, numbers, - and _. Used for its folder.">
                <Input
                  autoFocus
                  value={name}
                  placeholder="survival"
                  onChange={(e) => setName(e.target.value)}
                />
              </Field>
              <Field label="Port" hint="25565 is Minecraft's default. Each server on a PC needs its own.">
                <Input
                  type="number"
                  className="w-32 tabular-nums"
                  value={port}
                  onChange={(e) => setPort(Number(e.target.value) || 0)}
                />
              </Field>
              <Field label="Server program" hint="The moddedMC server, pack-dev.exe or a pack build.">
                <div className="flex gap-2">
                  <Input
                    value={exe}
                    placeholder="C:\\…\\pack-dev.exe"
                    onChange={(e) => setExe(e.target.value)}
                  />
                  <Button variant="outline" onClick={browse}>
                    <FolderOpen /> Browse
                  </Button>
                </div>
              </Field>
            </DialogPanel>
            <DialogFooter>
              <Button variant="ghost" onClick={() => onOpenChange(false)}>
                Cancel
              </Button>
              <Button disabled={!valid || busy} onClick={create}>
                Create
              </Button>
            </DialogFooter>
          </>
        ) : (
          <>
            <DialogHeader>
              <DialogTitle>How will friends join?</DialogTitle>
              <DialogDescription>
                {name} is ready. Pick how players reach it; you can come back to this any time.
              </DialogDescription>
            </DialogHeader>
            <DialogPanel className="flex flex-col gap-4">
              <div className="grid grid-cols-2 gap-2">
                {HOSTING.map((h) => (
                  <button
                    key={h.id}
                    onClick={() => setHosting(h.id)}
                    className={cn(
                      "flex flex-col gap-1 rounded-xl border p-3 text-left transition-colors",
                      hosting === h.id ? "border-primary bg-primary/8" : "hover:bg-accent",
                    )}
                  >
                    <span className="flex items-center gap-2 font-medium text-sm">
                      <h.icon className="size-4" /> {h.title}
                    </span>
                    <span className="text-muted-foreground text-xs">{h.blurb}</span>
                  </button>
                ))}
              </div>
              <ol className="flex flex-col gap-2 rounded-xl border bg-card p-4 text-sm">
                {guide.steps(port).map((s, i) => (
                  <li key={i} className="flex gap-3">
                    <span className="flex size-5 shrink-0 items-center justify-center rounded-full bg-muted font-medium text-xs">
                      {i + 1}
                    </span>
                    <span className="select-text">{s}</span>
                  </li>
                ))}
              </ol>
            </DialogPanel>
            <DialogFooter>
              <Button onClick={() => onOpenChange(false)}>Done</Button>
            </DialogFooter>
          </>
        )}
      </DialogPopup>
    </Dialog>
  );
}

function Field({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <label className="flex flex-col gap-1.5">
      <span className="font-medium text-sm">{label}</span>
      {children}
      {hint && <span className="text-muted-foreground text-xs">{hint}</span>}
    </label>
  );
}
