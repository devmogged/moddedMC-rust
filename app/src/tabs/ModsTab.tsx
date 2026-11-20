import { useEffect, useMemo, useState } from "react";
import { Boxes, Download, FileUp, Hammer, PackagePlus, Puzzle, Search, Trash2 } from "lucide-react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api, type Device, type ModEntry, type Pack, type ServerInfo } from "~/api";
import { Badge } from "~/components/ui/badge";
import { Button } from "~/components/ui/button";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "~/components/ui/empty";
import { Input } from "~/components/ui/input";
import { Select, SelectItem, SelectPopup, SelectTrigger, SelectValue } from "~/components/ui/select";
import { Skeleton } from "~/components/ui/skeleton";
import { errorText, formatBytes, usePoll } from "~/lib/hooks";
import { toast } from "~/lib/toast";
import { cn } from "~/lib/utils";
import { addModFiles } from "./servers/ModsPanel";

const inTauri = "__TAURI_INTERNALS__" in window;

type Target = { device: Device; server: ServerInfo };
const key = (t: Target) => `${t.device.id}/${t.server.name}`;

/** The mod marketplace: every published .mmc mod, installed onto the server picked at the top. */
export function ModsTab() {
  const mods = usePoll(api.mods, 10 * 60_000, []);
  const devices = usePoll(api.devices, 60_000, []);
  const [targets, setTargets] = useState<Target[]>([]);
  const [target, setTarget] = useState("");
  const [query, setQuery] = useState("");
  const [installing, setInstalling] = useState<string | null>(null);
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
      setTargets(flat);
      setTarget((t) => t || (flat[0] ? key(flat[0]) : ""));
    });
    return () => {
      alive = false;
    };
  }, [devices.value]);

  const chosen = targets.find((x) => key(x) === target);

  /** The picked server when it can take mods now; otherwise says why not. */
  const ready = (): Target | null => {
    if (!chosen) {
      toast("Make a server first, in the Servers tab.", "error");
      return null;
    }
    if (chosen.server.state !== "stopped" && chosen.server.state !== "crashed") {
      toast(`Stop ${chosen.server.name} before adding mods.`, "error");
      return null;
    }
    return chosen;
  };

  const importFiles = async (paths: string[]) => {
    const t = ready();
    if (!t) return;
    const n = await addModFiles(t.device.id, t.server.name, paths);
    if (n) toast(`Added ${n} mod${n === 1 ? "" : "s"} to ${t.server.name}.`);
  };

  const install = async (mod: ModEntry) => {
    const t = ready();
    if (!t) return;
    setInstalling(mod.id);
    try {
      await api.installMod(t.device.id, t.server.name, mod.id);
      toast(`Installed ${mod.name} on ${t.server.name}. Start the server to load it.`);
    } catch (err) {
      toast(errorText(err), "error");
    } finally {
      setInstalling(null);
    }
  };

  useEffect(() => {
    if (!inTauri) return;
    const off = getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === "enter" || e.payload.type === "over") setDragging(true);
      else if (e.payload.type === "leave") setDragging(false);
      else if (e.payload.type === "drop") {
        setDragging(false);
        importFiles(e.payload.paths);
      }
    });
    return () => {
      off.then((f) => f());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [target, targets]);

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return (mods.value ?? []).filter(
      (m) => !q || `${m.name} ${m.id} ${m.description ?? ""}`.toLowerCase().includes(q),
    );
  }, [mods.value, query]);

  return (
    <div className={cn("h-full overflow-y-auto", dragging && "bg-primary/5")}>
      <div className="mx-auto flex max-w-5xl flex-col gap-6 px-6 py-8">
        <div className="flex flex-wrap items-center gap-3">
          <h1 className="font-semibold text-xl">Mods</h1>
          {mods.value && <Badge variant="secondary">{mods.value.length}</Badge>}
          <div className="relative ml-auto w-60">
            <Search className="-translate-y-1/2 absolute top-1/2 left-2.5 size-4 text-muted-foreground" />
            <Input
              className="pl-8"
              placeholder="Search mods"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
          <span className="text-muted-foreground text-sm">Install to</span>
          <Select value={target} onValueChange={(v) => v && setTarget(v)}>
            <SelectTrigger className="w-44">
              <SelectValue>{(v: string) => (v ? v.split("/").pop() : "No servers")}</SelectValue>
            </SelectTrigger>
            <SelectPopup>
              {targets.map((x) => (
                <SelectItem key={key(x)} value={key(x)}>
                  {x.server.name} <span className="text-muted-foreground">· {x.device.name}</span>
                </SelectItem>
              ))}
            </SelectPopup>
          </Select>
          <Button
            variant="outline"
            title="Add your own .mmc file. You can also drop files anywhere on this page."
            onClick={async () => {
              const path = await api.pickModFile();
              if (path) importFiles([path]);
            }}
          >
            <FileUp /> Import .mmc
          </Button>
        </div>

        {mods.error && !mods.value ? (
          <p className="text-destructive-foreground text-sm">{mods.error}</p>
        ) : !mods.value ? (
          <div className="grid grid-cols-2 gap-3 lg:grid-cols-3">
            {Array.from({ length: 6 }, (_, i) => (
              <Skeleton key={i} className="h-36 rounded-xl" />
            ))}
          </div>
        ) : mods.value.length === 0 ? (
          <Empty className="rounded-xl border">
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <Puzzle />
              </EmptyMedia>
              <EmptyTitle>No mods published yet</EmptyTitle>
              <EmptyDescription>
                Finished ports appear here as they land in devmogged/mods. Meanwhile, import a .mmc file.
              </EmptyDescription>
            </EmptyHeader>
          </Empty>
        ) : shown.length === 0 ? (
          <p className="text-muted-foreground text-sm">No mods match “{query}”.</p>
        ) : (
          <div className="grid grid-cols-2 gap-3 lg:grid-cols-3">
            {shown.map((m) => (
              <article key={m.id} className="flex flex-col gap-3 rounded-xl border bg-card p-4">
                <div className="flex items-start gap-3">
                  {m.icon ? (
                    <img src={m.icon} alt="" className="size-10 shrink-0 rounded-lg" />
                  ) : (
                    <div className="flex size-10 shrink-0 items-center justify-center rounded-lg bg-primary/12 text-primary">
                      <Puzzle className="size-5" />
                    </div>
                  )}
                  <div className="flex min-w-0 flex-col">
                    <span className="truncate font-medium">{m.name}</span>
                    <span className="truncate text-muted-foreground text-xs">
                      {m.version}
                      {m.original ? ` · port of ${m.original}` : m.minecraft ? ` · from ${m.minecraft}` : ""}
                    </span>
                  </div>
                </div>
                {m.dependencies?.length > 0 && (
                  <p className="text-muted-foreground text-xs">Also installs {m.dependencies.join(", ")}</p>
                )}
                {m.description && (
                  <p className="line-clamp-3 text-muted-foreground text-sm">{m.description}</p>
                )}
                <div className="mt-auto flex items-center gap-2">
                  {m.download ? (
                    <Button size="sm" disabled={!!installing} onClick={() => install(m)}>
                      <Download className={cn(installing === m.id && "animate-pulse")} /> Install
                    </Button>
                  ) : (
                    <Badge variant="info" size="sm">
                      In packs only
                    </Badge>
                  )}
                  <span className="ml-auto truncate text-muted-foreground text-xs">
                    {[m.license, m.size ? formatBytes(m.size) : null].filter(Boolean).join(" · ")}
                  </span>
                </div>
              </article>
            ))}
          </div>
        )}

        <PacksSection />
      </div>
    </div>
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
