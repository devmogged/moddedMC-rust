import { useEffect, useState, type ComponentType } from "react";
import { Binary, Copy, Minus, Puzzle, Server, Settings as SettingsIcon, Square, X } from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import logo from "~/assets/logo.png";
import { Badge } from "~/components/ui/badge";
import { toast, Toaster } from "~/lib/toast";
import type { LaunchResult } from "./Launch";
import { cn } from "~/lib/utils";
import { ComingSoonTab } from "./tabs/ComingSoonTab";
import { ModsTab } from "./tabs/ModsTab";
import { ServersTab } from "./tabs/servers/ServersTab";
import { SettingsTab } from "./tabs/SettingsTab";

type TabId = "servers" | "mods" | "compiler" | "settings";

const TABS: { id: TabId; label: string; icon: ComponentType<{ className?: string }>; soon?: boolean }[] = [
  { id: "servers", label: "Servers", icon: Server },
  { id: "mods", label: "Mods", icon: Puzzle },
  { id: "compiler", label: "Jar → Rust", icon: Binary, soon: true },
  { id: "settings", label: "Settings", icon: SettingsIcon },
];

const TAB_KEY = "moddedmc.tab";
const inTauri = "__TAURI_INTERNALS__" in window;

function loadTab(): TabId {
  try {
    const saved = localStorage.getItem(TAB_KEY);
    if (TABS.some((t) => t.id === saved)) return saved as TabId;
  } catch {
    // No storage: start on Servers.
  }
  return "servers";
}

export function Shell({ launch }: { launch: LaunchResult }) {
  const [tab, setTab] = useState<TabId>(loadTab);

  useEffect(() => {
    if (launch.notice) toast(launch.notice, "error");
  }, [launch.notice]);

  const select = (next: TabId) => {
    setTab(next);
    try {
      localStorage.setItem(TAB_KEY, next);
    } catch {
      // Remembering the tab is a convenience only.
    }
  };

  // Ctrl+1..4 switch tabs, like T3 Code's workspace shortcuts.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const n = Number(e.key);
      if (e.ctrlKey && !e.shiftKey && !e.altKey && n >= 1 && n <= TABS.length) {
        e.preventDefault();
        select(TABS[n - 1].id);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="flex h-full flex-col bg-background text-foreground">
      <header data-tauri-drag-region className="flex h-11 shrink-0 items-center border-b pl-3">
        <div data-tauri-drag-region className="flex w-52 items-center gap-2">
          <img src={logo} alt="" className="pointer-events-none size-6" draggable={false} />
          <span data-tauri-drag-region className="font-semibold text-sm tracking-tight">
            moddedMC
          </span>
        </div>
        <nav
          role="tablist"
          aria-label="Sections"
          className="mx-auto flex items-center gap-0.5 rounded-lg bg-muted p-0.5"
        >
          {TABS.map(({ id, label, icon: Icon, soon }, i) => (
            <button
              key={id}
              role="tab"
              aria-selected={tab === id}
              title={`${label} (Ctrl+${i + 1})`}
              onClick={() => select(id)}
              className={cn(
                "flex h-7 items-center gap-1.5 rounded-md px-3 font-medium text-[13px] transition-colors",
                tab === id
                  ? "bg-accent text-foreground shadow-xs"
                  : "text-muted-foreground hover:text-foreground",
              )}
            >
              <Icon className="size-3.5" />
              {label}
              {soon && (
                <Badge variant="outline" size="sm">
                  Soon
                </Badge>
              )}
            </button>
          ))}
        </nav>
        <div data-tauri-drag-region className="flex h-full w-52 justify-end">
          {inTauri && <WindowControls />}
        </div>
      </header>
      <main key={tab} className="min-h-0 flex-1 animate-fade">
        {tab === "servers" && <ServersTab />}
        {tab === "mods" && <ModsTab />}
        {tab === "compiler" && <ComingSoonTab />}
        {tab === "settings" && <SettingsTab />}
      </main>
      <Toaster />
    </div>
  );
}

function WindowControls() {
  const win = getCurrentWindow();
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    win.isMaximized().then(setMaximized);
    const off = win.onResized(() => win.isMaximized().then(setMaximized));
    return () => {
      off.then((f) => f());
    };
  }, [win]);
  const button = "flex h-full w-12 items-center justify-center text-muted-foreground transition-colors";
  return (
    <>
      <button aria-label="Minimize" className={cn(button, "hover:bg-accent")} onClick={() => win.minimize()}>
        <Minus className="size-4" />
      </button>
      <button
        aria-label={maximized ? "Restore" : "Maximize"}
        className={cn(button, "hover:bg-accent")}
        onClick={() => win.toggleMaximize()}
      >
        {maximized ? <Copy className="size-3.5 -scale-x-100" /> : <Square className="size-3.5" />}
      </button>
      <button
        aria-label="Close"
        className={cn(button, "hover:bg-red-600 hover:text-white")}
        onClick={() => win.close()}
      >
        <X className="size-4" />
      </button>
    </>
  );
}
