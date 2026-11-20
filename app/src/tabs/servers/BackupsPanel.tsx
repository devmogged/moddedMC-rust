import { useState } from "react";
import { Archive, History, Trash2 } from "lucide-react";
import { api, type BackupSchedule, type Device, type ServerInfo } from "~/api";
import { Button } from "~/components/ui/button";
import { Select, SelectItem, SelectPopup, SelectTrigger, SelectValue } from "~/components/ui/select";
import { errorText, formatBytes, usePoll } from "~/lib/hooks";
import { toast } from "~/lib/toast";

const EVERY: { label: string; minutes: number | null }[] = [
  { label: "Off", minutes: null },
  { label: "Every hour", minutes: 60 },
  { label: "Every 6 hours", minutes: 360 },
  { label: "Every day", minutes: 1440 },
];

export function BackupsPanel({ device, server }: { device: Device; server: ServerInfo }) {
  const backups = usePoll(() => api.backups(device.id, server.name), 15_000, [device.id, server.name]);
  const schedule = usePoll(() => api.backupSchedule(device.id, server.name), 60_000, [
    device.id,
    server.name,
  ]);
  const [busy, setBusy] = useState(false);
  const stopped = server.state === "stopped" || server.state === "crashed";

  const run = async (work: () => Promise<unknown>, done: string) => {
    setBusy(true);
    try {
      await work();
      toast(done);
      backups.refresh();
      schedule.refresh();
    } catch (err) {
      toast(errorText(err), "error");
    } finally {
      setBusy(false);
    }
  };

  const setEvery = (minutes: number | null) => {
    const keep = schedule.value?.keep ?? 7;
    const next: BackupSchedule | null = minutes ? { every_minutes: minutes, keep } : null;
    run(
      () => api.setBackupSchedule(device.id, server.name, next),
      minutes ? "Automatic backups on." : "Automatic backups off.",
    );
  };

  return (
    <div className="flex flex-col gap-4 p-6">
      <div className="flex flex-wrap items-center gap-3">
        <Button disabled={busy} onClick={() => run(() => api.backUp(device.id, server.name), "Backed up.")}>
          <Archive /> Back up now
        </Button>
        <div className="ml-auto flex items-center gap-2 text-sm">
          <span className="text-muted-foreground">Automatic</span>
          <Select
            value={String(schedule.value?.every_minutes ?? "off")}
            onValueChange={(v) => setEvery(v === "off" ? null : Number(v))}
          >
            <SelectTrigger className="w-40">
              <SelectValue>
                {(v: string) =>
                  EVERY.find((e) => String(e.minutes ?? "off") === v)?.label ?? `Every ${v} min`
                }
              </SelectValue>
            </SelectTrigger>
            <SelectPopup>
              {EVERY.map((e) => (
                <SelectItem key={e.label} value={String(e.minutes ?? "off")}>
                  {e.label}
                </SelectItem>
              ))}
            </SelectPopup>
          </Select>
        </div>
      </div>
      <div className="divide-y rounded-xl border bg-card">
        {backups.value?.length === 0 && <p className="p-4 text-muted-foreground text-sm">No backups yet.</p>}
        {backups.value?.map((b) => (
          <div key={b.id} className="flex items-center gap-3 px-4 py-2.5">
            <History className="size-4 text-muted-foreground" />
            <div className="flex flex-1 flex-col">
              <span className="font-medium text-sm">{new Date(b.created * 1000).toLocaleString()}</span>
              <span className="text-muted-foreground text-xs">
                {b.files} files · {formatBytes(b.bytes)}
                {b.scheduled ? " · automatic" : ""}
              </span>
            </div>
            <Button
              size="sm"
              variant="outline"
              disabled={busy || !stopped}
              title={stopped ? undefined : "Stop the server to restore"}
              onClick={() => run(() => api.restoreBackup(device.id, server.name, b.id), "Restored.")}
            >
              Restore
            </Button>
            <Button
              size="icon-sm"
              variant="ghost-destructive"
              aria-label="Delete backup"
              disabled={busy}
              onClick={() => run(() => api.removeBackup(device.id, server.name, b.id), "Backup deleted.")}
            >
              <Trash2 />
            </Button>
          </div>
        ))}
      </div>
      {backups.error && <p className="text-destructive-foreground text-sm">{backups.error}</p>}
    </div>
  );
}
