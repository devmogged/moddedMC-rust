import { useEffect, useRef, useState, type ReactNode } from "react";
import { ImagePlus, Trash2 } from "lucide-react";
import { api, type Device, type ServerConfig, type ServerInfo } from "~/api";
import { Button } from "~/components/ui/button";
import {
  Dialog,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogPopup,
  DialogTitle,
} from "~/components/ui/dialog";
import { Input } from "~/components/ui/input";
import { Select, SelectItem, SelectPopup, SelectTrigger, SelectValue } from "~/components/ui/select";
import { Spinner } from "~/components/ui/spinner";
import { Switch } from "~/components/ui/switch";
import { errorText } from "~/lib/hooks";
import { toast } from "~/lib/toast";

const DIFFICULTIES: ServerConfig["difficulty"][] = ["Peaceful", "Easy", "Normal", "Hard"];
const GAMEMODES: ServerConfig["gamemode"][] = ["Survival", "Creative", "Adventure", "Spectator"];

export function SettingsPanel({
  device,
  server,
  lifecycleBusy,
  onPortSaved,
  onDeleted,
}: {
  device: Device;
  server: ServerInfo;
  lifecycleBusy: boolean;
  onPortSaved: (port: number) => void;
  onDeleted: () => void;
}) {
  const port = server.port ?? 25565;
  const [editedPort, setDraftPort] = useState<string | null>(null);
  const draftPort = editedPort ?? String(port);
  const [savingPort, setSavingPort] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const stopped = server.state === "stopped" || server.state === "crashed";
  const locked = !stopped || lifecycleBusy;
  const nextPort = Number(draftPort);
  const validPort = /^\d+$/.test(draftPort) && nextPort >= 1 && nextPort <= 65535;

  const savePort = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (locked || savingPort || !validPort || nextPort === port) return;
    setSavingPort(true);
    try {
      await api.setServerPort(device.id, server.name, nextPort);
      onPortSaved(nextPort);
      setDraftPort(null);
      toast("Java port saved.");
    } catch (err) {
      toast(errorText(err), "error");
    } finally {
      setSavingPort(false);
    }
  };

  const deleteServer = async () => {
    setDeleting(true);
    setDeleteError(null);
    try {
      await api.deleteServer(device.id, server.name);
      toast(`Deleted ${server.name}.`);
      onDeleted();
    } catch (err) {
      setDeleteError(errorText(err));
      setDeleting(false);
    }
  };

  return (
    <div className="flex flex-col gap-6 p-6 pb-24">
      <Group title="Connection">
        <form onSubmit={savePort}>
          <Row
            label="Java port"
            hint={
              locked
                ? "Stop the server to change its port."
                : "The server uses this port the next time it starts. The default is 25565."
            }
          >
            <div className="flex items-center gap-2">
              <Input
                aria-label="Java port"
                type="number"
                min={1}
                max={65535}
                step={1}
                className="w-24 text-right tabular-nums"
                value={draftPort}
                disabled={locked || savingPort}
                onChange={(event) => setDraftPort(event.target.value)}
              />
              <Button
                size="sm"
                type="submit"
                disabled={locked || savingPort || !validPort || nextPort === port}
              >
                {savingPort ? "Saving..." : "Save port"}
              </Button>
            </div>
          </Row>
        </form>
      </Group>

      <GeneralSettings device={device} server={server} />

      <Group title="Danger zone" danger>
        <Row
          label="Delete server"
          hint={
            locked
              ? "Stop the server before deleting it."
              : "Permanently delete this server, its worlds, mods, settings, and backups."
          }
        >
          <Button
            variant="destructive-outline"
            disabled={locked || savingPort}
            onClick={() => {
              setDeleteError(null);
              setConfirming(true);
            }}
          >
            <Trash2 /> Delete server
          </Button>
        </Row>
      </Group>
      <Dialog open={confirming} onOpenChange={(open) => !deleting && setConfirming(open)}>
        <DialogPopup showCloseButton={false}>
          <DialogHeader>
            <DialogTitle>Delete {server.name}?</DialogTitle>
            <DialogDescription>
              This permanently deletes all files and backups for this server, including its worlds.
            </DialogDescription>
            {deleteError && (
              <p role="alert" className="text-destructive-foreground text-sm">
                {deleteError}
              </p>
            )}
          </DialogHeader>
          <DialogFooter>
            <Button
              variant="ghost"
              data-slot="dialog-action"
              disabled={deleting}
              onClick={() => setConfirming(false)}
            >
              Cancel
            </Button>
            <Button variant="destructive" disabled={deleting || locked} onClick={deleteServer}>
              {deleting ? "Deleting..." : "Delete permanently"}
            </Button>
          </DialogFooter>
        </DialogPopup>
      </Dialog>
    </div>
  );
}

function GeneralSettings({ device, server }: { device: Device; server: ServerInfo }) {
  const [saved, setSaved] = useState<ServerConfig | null>(null);
  const [draft, setDraft] = useState<ServerConfig | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const fileRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    api.config(device.id, server.name).then(
      (c) => {
        setSaved(c);
        setDraft(c);
      },
      (err) => setError(errorText(err)),
    );
  }, [device.id, server.name]);

  if (error) return <p className="text-destructive-foreground text-sm">{error}</p>;
  if (!draft || !saved)
    return (
      <div className="flex justify-center p-10">
        <Spinner />
      </div>
    );

  const set = <K extends keyof ServerConfig>(key: K, value: ServerConfig[K]) =>
    setDraft({ ...draft, [key]: value });
  const dirty = JSON.stringify(draft) !== JSON.stringify(saved);
  const running = server.state === "running" || server.state === "starting";

  const save = async (restart: boolean) => {
    setSaving(true);
    try {
      await api.setConfig(device.id, server.name, draft);
      setSaved(draft);
      if (restart) await api.action(device.id, server.name, "restart");
      toast(restart ? "Saved. Restarting the server." : running ? "Saved. Restart to apply." : "Saved.");
    } catch (err) {
      toast(errorText(err), "error");
    } finally {
      setSaving(false);
    }
  };

  const pickIcon = async (file: File) => {
    try {
      set("icon", await toIcon(file));
    } catch (err) {
      toast(errorText(err), "error");
    }
  };

  return (
    <>
      <Group title="Who can join">
        <Row label="Java Edition" hint="Players on PC with the normal Minecraft launcher.">
          <Switch checked={draft.java} onCheckedChange={(v) => set("java", v)} />
        </Row>
        <Row label="Bedrock Edition" hint="Phones, consoles and Windows 10/11 edition.">
          <Switch checked={draft.bedrock} onCheckedChange={(v) => set("bedrock", v)} />
        </Row>
        <Row
          label="Online mode"
          hint="Only players with a paid Microsoft account. Turn off for TLauncher and other launchers."
        >
          <Switch checked={draft.online_mode} onCheckedChange={(v) => set("online_mode", v)} />
        </Row>
        <Row label="Whitelist" hint="Only players you add with /whitelist add can join.">
          <Switch checked={draft.white_list} onCheckedChange={(v) => set("white_list", v)} />
        </Row>
        <Row label="Max players">
          <NumberInput
            value={draft.max_players}
            min={1}
            max={10000}
            onChange={(v) => set("max_players", v)}
          />
        </Row>
      </Group>

      <Group title="Server list">
        <Row label="Icon" hint="Shown next to the server in Multiplayer. Square images work best.">
          <div className="flex items-center gap-2">
            {draft.icon ? (
              <img
                src={`data:image/png;base64,${toBase64(draft.icon)}`}
                alt=""
                className="size-10 rounded-md border [image-rendering:pixelated]"
              />
            ) : (
              <div className="size-10 rounded-md border border-dashed" />
            )}
            <Button size="sm" variant="outline" onClick={() => fileRef.current?.click()}>
              <ImagePlus /> Choose
            </Button>
            {draft.icon && (
              <Button
                size="icon-sm"
                variant="ghost"
                aria-label="Remove icon"
                onClick={() => set("icon", null)}
              >
                <Trash2 />
              </Button>
            )}
            <input
              ref={fileRef}
              type="file"
              accept="image/*"
              hidden
              onChange={(e) => {
                const f = e.target.files?.[0];
                if (f) pickIcon(f);
                e.target.value = "";
              }}
            />
          </div>
        </Row>
        <div className="flex flex-col gap-1.5 px-4 py-3">
          <span className="font-medium text-sm">Message of the day</span>
          <Input value={draft.motd} maxLength={256} onChange={(e) => set("motd", e.target.value)} />
          <span className="text-muted-foreground text-xs">
            The line under the server's name in Multiplayer.
          </span>
        </div>
      </Group>

      <Group title="Gameplay">
        <Row label="Game mode">
          <Choice value={draft.gamemode} options={GAMEMODES} onChange={(v) => set("gamemode", v)} />
        </Row>
        <Row label="Difficulty">
          <Choice value={draft.difficulty} options={DIFFICULTIES} onChange={(v) => set("difficulty", v)} />
        </Row>
        <Row label="Hardcore" hint="One life: players become spectators when they die.">
          <Switch checked={draft.hardcore} onCheckedChange={(v) => set("hardcore", v)} />
        </Row>
        <Row label="The Nether">
          <Switch checked={draft.allow_nether} onCheckedChange={(v) => set("allow_nether", v)} />
        </Row>
        <Row label="The End">
          <Switch checked={draft.allow_end} onCheckedChange={(v) => set("allow_end", v)} />
        </Row>
        <Row label="Spawn protection" hint="Blocks around spawn only operators can change.">
          <NumberInput
            value={draft.spawn_protection}
            min={0}
            max={1000}
            onChange={(v) => set("spawn_protection", v)}
          />
        </Row>
      </Group>

      <Group title="Performance">
        <Row label="View distance" hint="Chunks each player sees. Lower is lighter.">
          <NumberInput
            value={draft.view_distance}
            min={2}
            max={32}
            onChange={(v) => set("view_distance", v)}
          />
        </Row>
        <Row label="Simulation distance" hint="Chunks around players where things move and grow.">
          <NumberInput
            value={draft.simulation_distance}
            min={2}
            max={32}
            onChange={(v) => set("simulation_distance", v)}
          />
        </Row>
      </Group>

      {dirty && (
        <div className="dropdown-glass fixed right-6 bottom-6 z-40 flex animate-rise items-center gap-3 rounded-xl p-3 pl-4 shadow-lg">
          <span className="text-sm">Unsaved changes</span>
          <Button size="sm" variant="ghost" disabled={saving} onClick={() => setDraft(saved)}>
            Discard
          </Button>
          {running && (
            <Button size="sm" variant="outline" disabled={saving} onClick={() => save(true)}>
              Save and restart
            </Button>
          )}
          <Button size="sm" disabled={saving} onClick={() => save(false)}>
            Save
          </Button>
        </div>
      )}
    </>
  );
}

function Group({ title, children, danger }: { title: string; children: ReactNode; danger?: boolean }) {
  return (
    <section className="flex flex-col gap-2">
      <h2 className="px-1 font-medium text-muted-foreground text-xs uppercase tracking-wide">{title}</h2>
      <div
        className={
          danger
            ? "divide-y rounded-xl border border-destructive/40 bg-card"
            : "divide-y rounded-xl border bg-card"
        }
      >
        {children}
      </div>
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

function NumberInput({
  value,
  min,
  max,
  onChange,
}: {
  value: number;
  min: number;
  max: number;
  onChange: (v: number) => void;
}) {
  return (
    <Input
      type="number"
      className="w-24 text-right tabular-nums"
      value={value}
      min={min}
      max={max}
      onChange={(e) => {
        const n = Math.round(Number(e.target.value));
        if (Number.isFinite(n)) onChange(Math.min(max, Math.max(min, n)));
      }}
    />
  );
}

function Choice<T extends string>({
  value,
  options,
  onChange,
}: {
  value: T;
  options: T[];
  onChange: (v: T) => void;
}) {
  return (
    <Select value={value} onValueChange={(v) => v && onChange(v as T)}>
      <SelectTrigger className="w-36">
        <SelectValue />
      </SelectTrigger>
      <SelectPopup>
        {options.map((o) => (
          <SelectItem key={o} value={o}>
            {o}
          </SelectItem>
        ))}
      </SelectPopup>
    </Select>
  );
}

function toBase64(bytes: number[]): string {
  let s = "";
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s);
}

/** Any image the browser can read, as the 64x64 PNG Minecraft wants. */
async function toIcon(file: File): Promise<number[]> {
  const bitmap = await createImageBitmap(file);
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = 64;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("Can't read that image.");
  const side = Math.min(bitmap.width, bitmap.height);
  ctx.drawImage(bitmap, (bitmap.width - side) / 2, (bitmap.height - side) / 2, side, side, 0, 0, 64, 64);
  const blob = await new Promise<Blob | null>((r) => canvas.toBlob(r, "image/png"));
  if (!blob) throw new Error("Can't convert that image.");
  return Array.from(new Uint8Array(await blob.arrayBuffer()));
}
