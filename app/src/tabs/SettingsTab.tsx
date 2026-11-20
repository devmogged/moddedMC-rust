import { useEffect, useState, type ReactNode } from "react";
import { HardDrive, Monitor, Moon, Plus, Sun, Trash2 } from "lucide-react";
import { api, type Route } from "~/api";
import { Button } from "~/components/ui/button";
import { Input } from "~/components/ui/input";
import { Select, SelectItem, SelectPopup, SelectTrigger, SelectValue } from "~/components/ui/select";
import { errorText, usePoll } from "~/lib/hooks";
import { toast } from "~/lib/toast";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { Switch } from "~/components/ui/switch";
import { ToggleGroup, ToggleGroupItem } from "~/components/ui/toggle-group";
import { setSetting, useSettings, type ThemeChoice } from "~/settings";

const inTauri = "__TAURI_INTERNALS__" in window;

export function SettingsTab() {
  const settings = useSettings();
  const [startup, setStartup] = useState<boolean | null>(null);

  useEffect(() => {
    if (inTauri) isEnabled().then(setStartup, () => setStartup(null));
  }, []);

  const toggleStartup = async (on: boolean) => {
    setStartup(on);
    try {
      await (on ? enable() : disable());
    } catch {
      setStartup(!on);
    }
  };

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto flex max-w-2xl flex-col gap-8 px-6 py-8">
        <h1 className="font-semibold text-xl">Settings</h1>

        <Section title="Appearance">
          <Row label="Theme" hint="Follow Windows, or pick one.">
            <ToggleGroup
              value={[settings.theme]}
              onValueChange={(value) => value[0] && setSetting("theme", value[0] as ThemeChoice)}
              variant="outline"
              size="sm"
            >
              <ToggleGroupItem value="system" aria-label="System">
                <Monitor /> System
              </ToggleGroupItem>
              <ToggleGroupItem value="light" aria-label="Light">
                <Sun /> Light
              </ToggleGroupItem>
              <ToggleGroupItem value="dark" aria-label="Dark">
                <Moon /> Dark
              </ToggleGroupItem>
            </ToggleGroup>
          </Row>
          <Row label="Reduce motion" hint="Skip the launch animation and most transitions.">
            <Switch
              checked={settings.reduceMotion}
              onCheckedChange={(on) => setSetting("reduceMotion", on)}
            />
          </Row>
        </Section>

        <Section title="App">
          <Row
            label="Run on startup"
            hint={inTauri ? "Open moddedMC when you sign in to Windows." : "Only in the installed app."}
          >
            <Switch checked={startup ?? false} disabled={startup === null} onCheckedChange={toggleStartup} />
          </Row>
          <Row label="Check for updates on launch" hint="New versions come from GitHub Releases.">
            <Switch checked={settings.autoUpdate} onCheckedChange={(on) => setSetting("autoUpdate", on)} />
          </Row>
        </Section>

        <DevicesSection />
      </div>
    </div>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-2">
      <h2 className="px-1 font-medium text-muted-foreground text-xs uppercase tracking-wide">{title}</h2>
      <div className="divide-y rounded-xl border bg-card">{children}</div>
    </section>
  );
}

function Row({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-6 px-4 py-3">
      <div className="flex flex-col gap-0.5">
        <span className="font-medium text-sm">{label}</span>
        {hint && <span className="text-muted-foreground text-xs">{hint}</span>}
      </div>
      {children}
    </div>
  );
}

const ROUTES: { id: Exclude<Route, "local">; label: string; host: string }[] = [
  { id: "tailscale", label: "Tailscale", host: "machine name or 100.x address" },
  { id: "ssh", label: "SSH", host: "user@host" },
];

/** Other machines running the moddedMC agent, such as a Raspberry Pi. */
function DevicesSection() {
  const devices = usePoll(api.devices, 30_000, []);
  const [route, setRoute] = useState<"tailscale" | "ssh">("tailscale");
  const [name, setName] = useState("");
  const [host, setHost] = useState("");

  const add = async () => {
    try {
      await api.addDevice({ name: name.trim(), route, host: host.trim() });
      setName("");
      setHost("");
      devices.refresh();
    } catch (err) {
      toast(errorText(err), "error");
    }
  };

  const remove = async (id: string) => {
    try {
      await api.removeDevice(id);
      devices.refresh();
    } catch (err) {
      toast(errorText(err), "error");
    }
  };

  return (
    <Section title="Devices">
      {devices.value?.map((d) => (
        <div key={d.id} className="flex items-center gap-3 px-4 py-3">
          <HardDrive className="size-4 text-muted-foreground" />
          <div className="flex flex-1 flex-col">
            <span className="font-medium text-sm">{d.name}</span>
            <span className="text-muted-foreground text-xs">
              {d.route === "local" ? "This PC" : `${d.route === "ssh" ? "SSH" : "Tailscale"} · ${d.host}`}
            </span>
          </div>
          {d.route !== "local" && (
            <Button
              size="icon-sm"
              variant="ghost-destructive"
              aria-label={`Remove ${d.name}`}
              onClick={() => remove(d.id)}
            >
              <Trash2 />
            </Button>
          )}
        </div>
      ))}
      <div className="flex flex-wrap items-center gap-2 px-4 py-3">
        <Input className="w-36" placeholder="Name" value={name} onChange={(e) => setName(e.target.value)} />
        <Select value={route} onValueChange={(v) => v && setRoute(v as "tailscale" | "ssh")}>
          <SelectTrigger className="w-32">
            <SelectValue>{(v: string) => ROUTES.find((r) => r.id === v)?.label}</SelectValue>
          </SelectTrigger>
          <SelectPopup>
            {ROUTES.map((r) => (
              <SelectItem key={r.id} value={r.id}>
                {r.label}
              </SelectItem>
            ))}
          </SelectPopup>
        </Select>
        <Input
          className="min-w-40 flex-1"
          placeholder={ROUTES.find((r) => r.id === route)?.host}
          value={host}
          onChange={(e) => setHost(e.target.value)}
        />
        <Button size="sm" variant="outline" disabled={!name.trim() || !host.trim()} onClick={add}>
          <Plus /> Add
        </Button>
      </div>
    </Section>
  );
}
