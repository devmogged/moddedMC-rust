import { useEffect, useState } from "react";
import { Plus, Puzzle, Trash2 } from "lucide-react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api, type Device, type ServerInfo } from "~/api";
import { Button } from "~/components/ui/button";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "~/components/ui/empty";
import { errorText, formatBytes, usePoll } from "~/lib/hooks";
import { toast } from "~/lib/toast";
import { cn } from "~/lib/utils";

const inTauri = "__TAURI_INTERNALS__" in window;

/** Add the `.mmc` and `.jar` files at `paths` to a stopped server; returns how many went in. */
export async function addModFiles(device: string, server: string, paths: string[]): Promise<number> {
  let added = 0;
  for (const path of paths) {
    if (!/\.(mmc|jar)$/i.test(path)) {
      toast(`${path.split(/[\\/]/).pop()} isn't a .mmc or .jar mod.`, "error");
      continue;
    }
    try {
      await api.addMod(device, server, path);
      added++;
    } catch (err) {
      toast(errorText(err), "error");
    }
  }
  return added;
}

export function ModsPanel({ device, server }: { device: Device; server: ServerInfo }) {
  const mods = usePoll(() => api.serverMods(device.id, server.name), 10_000, [device.id, server.name]);
  const [dragging, setDragging] = useState(false);
  const stopped = server.state === "stopped" || server.state === "crashed";

  useEffect(() => {
    if (!inTauri) return;
    const off = getCurrentWebview().onDragDropEvent(async (e) => {
      if (e.payload.type === "enter" || e.payload.type === "over") setDragging(true);
      else if (e.payload.type === "leave") setDragging(false);
      else if (e.payload.type === "drop") {
        setDragging(false);
        if (!stopped) return toast("Stop the server before adding mods.", "error");
        const n = await addModFiles(device.id, server.name, e.payload.paths);
        if (n) toast(`Added ${n} mod${n === 1 ? "" : "s"}. Start the server to load them.`);
        mods.refresh();
      }
    });
    return () => {
      off.then((f) => f());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [device.id, server.name, stopped]);

  const add = async () => {
    const path = await api.pickModFile();
    if (!path) return;
    if (await addModFiles(device.id, server.name, [path])) toast("Mod added.");
    mods.refresh();
  };

  const remove = async (file: string) => {
    try {
      await api.removeMod(device.id, server.name, file);
      mods.refresh();
    } catch (err) {
      toast(errorText(err), "error");
    }
  };

  return (
    <div
      className={cn(
        "flex min-h-full flex-col gap-4 p-6",
        dragging && "bg-primary/5 outline-2 outline-dashed outline-primary/50 -outline-offset-8",
      )}
    >
      <div className="flex items-center justify-between">
        <p className="text-muted-foreground text-sm">
          {stopped
            ? "Drop .mmc or Fabric .jar files here, or add one."
            : "Stop the server to add or remove mods."}
        </p>
        <Button size="sm" variant="outline" disabled={!stopped} onClick={add}>
          <Plus /> Add mod
        </Button>
      </div>
      {mods.value?.length === 0 ? (
        <Empty>
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <Puzzle />
            </EmptyMedia>
            <EmptyTitle>No mods yet</EmptyTitle>
            <EmptyDescription>Find ported mods in the Mods tab, or drop a mod file here.</EmptyDescription>
          </EmptyHeader>
        </Empty>
      ) : (
        <div className="divide-y rounded-xl border bg-card">
          {mods.value?.map((m) => (
            <div key={m.file} className="flex items-center gap-3 px-4 py-2.5">
              <Puzzle className="size-4 text-muted-foreground" />
              <div className="flex min-w-0 flex-1 flex-col">
                <span className="truncate font-medium text-sm">{m.id ?? m.file}</span>
                <span className="text-muted-foreground text-xs">
                  {[m.version, m.file, formatBytes(m.bytes)].filter(Boolean).join(" · ")}
                </span>
              </div>
              <Button
                size="icon-sm"
                variant="ghost-destructive"
                aria-label={`Remove ${m.file}`}
                disabled={!stopped}
                onClick={() => remove(m.file)}
              >
                <Trash2 />
              </Button>
            </div>
          ))}
        </div>
      )}
      {mods.error && <p className="text-destructive-foreground text-sm">{mods.error}</p>}
    </div>
  );
}
