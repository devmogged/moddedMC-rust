import { useEffect, useMemo, useState } from "react";
import { Boxes, Hammer, PackagePlus, Puzzle, Search, Trash2, Upload } from "lucide-react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api, type Device, type Pack, type ServerInfo } from "~/api";
import { Badge } from "~/components/ui/badge";
import { Button } from "~/components/ui/button";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "~/components/ui/empty";
import { Input } from "~/components/ui/input";
import { Select, SelectItem, SelectPopup, SelectTrigger, SelectValue } from "~/components/ui/select";
import { Skeleton } from "~/components/ui/skeleton";
import { errorText, usePoll } from "~/lib/hooks";
import { toast } from "~/lib/toast";
import { cn } from "~/lib/utils";
import { addModFiles } from "./servers/ModsPanel";

const inTauri = "__TAURI_INTERNALS__" in window;

export function ModsTab() {
  const mods = usePoll(api.mods, 10 * 60_000, []);
  const [query, setQuery] = useState("");
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return (mods.value ?? []).filter((m) => !q || `${m.name} ${m.id}`.toLowerCase().includes(q));
  }, [mods.value, query]);

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto flex max-w-5xl flex-col gap-8 px-6 py-8">
        <InstallBox />

        <section className="flex flex-col gap-4">
          <div className="flex items-center gap-3">
            <h2 className="font-semibold text-lg">Ported mods</h2>
            {mods.value && <Badge variant="secondary">{mods.value.length}</Badge>}
            <div className="relative ml-auto w-64">
              <Search className="-translate-y-1/2 absolute top-1/2 left-2.5 size-4 text-muted-foreground" />
              <Input
                className="pl-8"
                placeholder="Search mods"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
            </div>
          </div>
          {mods.error && !mods.value ? (
            <p className="text-destructive-foreground text-sm">{mods.error}</p>
          ) : !mods.value ? (
            <div className="grid grid-cols-2 gap-3 lg:grid-cols-3">
              {Array.from({ length: 6 }, (_, i) => (
                <Skeleton key={i} className="h-24 rounded-xl" />
              ))}
            </div>
          ) : shown.length === 0 ? (
            <p className="text-muted-foreground text-sm">No mods match “{query}”.</p>
          ) : (
            <div className="grid grid-cols-2 gap-3 lg:grid-cols-3">
              {shown.map((m) => (
                <article key={m.id} className="flex flex-col gap-2 rounded-xl border bg-card p-4">
                  <div className="flex items-start gap-3">
                    <div className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-primary/12 text-primary">
                      <Puzzle className="size-4.5" />
                    </div>
                    <div className="flex min-w-0 flex-col">
                      <span className="truncate font-medium text-sm">{m.name}</span>
                      <span className="truncate text-muted-foreground text-xs">
                        {m.version}
                        {m.minecraft ? ` · from ${m.minecraft}` : ""}
                      </span>
                    </div>
                  </div>
                  <div className="mt-auto flex items-center gap-2">
                    <Badge variant={m.kind === "port" ? "info" : "success"} size="sm">
                      {m.kind === "port" ? "Built in" : "Drop-in"}
                    </Badge>
                    {m.upstream && (
                      <span className="truncate text-muted-foreground text-xs">{m.upstream}</span>
                    )}
                  </div>
                </article>
              ))}
            </div>
          )}
        </section>

        <PacksSection />
      </div>
    </div>
  );
}

/** Drop mod files (or pick one) and send them to a stopped server. */
function InstallBox() {
  const devices = usePoll(api.devices, 60_000, []);
  const [servers, setServers] = useState<{ device: Device; server: ServerInfo }[]>([]);
  const [target, setTarget] = useState("");
  const [dragging, setDragging] = useState(false);

  useEffect(() => {
    let alive = true;
    Promise.all(
      (devices.value ?? []).map((d) =>
        api.servers(d.id).then(
          (list) => list.filter((s) => !s.external).map((server) => ({ device: d, server })),
          () => [],
        ),
      ),
    ).then((all) => {
      if (!alive) return;
      const flat = all.flat();
      setServers(flat);
      setTarget((t) => t || (flat[0] ? `${flat[0].device.id}/${flat[0].server.name}` : ""));
    });
    return () => {
      alive = false;
    };
  }, [devices.value]);

  const send = async (paths: string[]) => {
    const chosen = servers.find((x) => `${x.device.id}/${x.server.name}` === target);
    if (!chosen) return toast("Make a server first, in the Servers tab.", "error");
    if (chosen.server.state !== "stopped" && chosen.server.state !== "crashed")
      return toast(`Stop ${chosen.server.name} before adding mods.`, "error");
    const n = await addModFiles(chosen.device.id, chosen.server.name, paths);
    if (n) toast(`Added ${n} mod${n === 1 ? "" : "s"} to ${chosen.server.name}.`);
  };

  useEffect(() => {
    if (!inTauri) return;
    const off = getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === "enter" || e.payload.type === "over") setDragging(true);
      else if (e.payload.type === "leave") setDragging(false);
      else if (e.payload.type === "drop") {
        setDragging(false);
        send(e.payload.paths);
      }
    });
    return () => {
      off.then((f) => f());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [target, servers]);

  return (
    <section
      className={cn(
        "flex items-center gap-4 rounded-2xl border border-dashed p-5 transition-colors",
        dragging ? "border-primary bg-primary/8" : "bg-card",
      )}
    >
      <div className="flex size-11 items-center justify-center rounded-xl bg-muted">
        <Upload className="size-5" />
      </div>
      <div className="flex flex-1 flex-col">
        <span className="font-medium text-sm">Drop .mmc mods here</span>
        <span className="text-muted-foreground text-xs">
          They go into the server you pick, ready for its next start.
        </span>
      </div>
      <Select value={target} onValueChange={(v) => v && setTarget(v)}>
        <SelectTrigger className="w-48">
          <SelectValue>{(v: string) => (v ? v.split("/").pop() : "No servers")}</SelectValue>
        </SelectTrigger>
        <SelectPopup>
          {servers.map((x) => (
            <SelectItem key={`${x.device.id}/${x.server.name}`} value={`${x.device.id}/${x.server.name}`}>
              {x.server.name} <span className="text-muted-foreground">· {x.device.name}</span>
            </SelectItem>
          ))}
        </SelectPopup>
      </Select>
      <Button
        variant="outline"
        disabled={!target}
        onClick={async () => {
          const path = await api.pickModFile();
          if (path) send([path]);
        }}
      >
        Choose file
      </Button>
    </section>
  );
}

function PacksSection() {
  const packs = usePoll(api.packs, 30_000, []);
  const [log, setLog] = useState<{ path: string; lines: string[] } | null>(null);
  const [building, setBuilding] = useState<string | null>(null);

  useEffect(() => {
    const off = api.onPackBuild((e) =>
      setLog((l) => (l && l.path === e.path ? { ...l, lines: [...l.lines.slice(-300), e.line] } : l)),
    );
    return () => {
      off.then((f) => f());
    };
  }, []);

  const add = async () => {
    const path = await api.pickPackFile();
    if (!path) return;
    try {
      await api.addPack(path);
      packs.refresh();
    } catch (err) {
      toast(errorText(err), "error");
    }
  };

  const build = async (pack: Pack, target: string) => {
    setBuilding(`${pack.path}/${target}`);
    setLog({ path: pack.path, lines: [] });
    try {
      const out = await api.buildPack(pack.path, target);
      toast(`Built ${pack.name} for ${target}: ${out}`);
      packs.refresh();
    } catch (err) {
      toast(errorText(err), "error");
    } finally {
      setBuilding(null);
    }
  };

  const remove = async (pack: Pack) => {
    try {
      await api.removePack(pack.path);
      packs.refresh();
    } catch (err) {
      toast(errorText(err), "error");
    }
  };

  return (
    <section className="flex flex-col gap-4">
      <div className="flex items-center gap-3">
        <h2 className="font-semibold text-lg">Packs</h2>
        <span className="text-muted-foreground text-sm">A server binary with its mods built in.</span>
        <Button size="sm" variant="outline" className="ml-auto" onClick={add}>
          <PackagePlus /> Add pack
        </Button>
      </div>
      {packs.value?.length === 0 ? (
        <Empty className="rounded-xl border">
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <Boxes />
            </EmptyMedia>
            <EmptyTitle>No packs</EmptyTitle>
            <EmptyDescription>Add a pack.toml to build a server with its mods compiled in.</EmptyDescription>
          </EmptyHeader>
        </Empty>
      ) : (
        <div className="divide-y rounded-xl border bg-card">
          {packs.value?.map((p) => (
            <div key={p.path} className="flex flex-col gap-2 px-4 py-3">
              <div className="flex items-center gap-3">
                <Boxes className="size-4 text-muted-foreground" />
                <div className="flex min-w-0 flex-1 flex-col">
                  <span className="font-medium text-sm">
                    {p.name} <span className="text-muted-foreground">v{p.version}</span>
                  </span>
                  <span className="truncate text-muted-foreground text-xs">
                    {p.error ?? `${p.mods.length} mods · ${p.path}`}
                  </span>
                </div>
                {p.targets.map((t) => (
                  <Button
                    key={t.name}
                    size="sm"
                    variant="outline"
                    disabled={!!building}
                    title={t.built ? `Last built ${t.built}` : undefined}
                    onClick={() => build(p, t.name)}
                  >
                    <Hammer className={cn(building === `${p.path}/${t.name}` && "animate-pulse")} /> {t.name}
                  </Button>
                ))}
                <Button
                  size="icon-sm"
                  variant="ghost-destructive"
                  aria-label={`Remove ${p.name}`}
                  onClick={() => remove(p)}
                >
                  <Trash2 />
                </Button>
              </div>
              {log?.path === p.path && log.lines.length > 0 && (
                <pre className="max-h-48 overflow-y-auto rounded-lg bg-muted p-2 font-mono text-xs select-text">
                  {log.lines.join("\n")}
                </pre>
              )}
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
