import { useState, type ComponentType } from "react";
import { Binary, Puzzle, Server, Settings as SettingsIcon } from "lucide-react";
import logo from "~/assets/logo.png";
import { Badge } from "~/components/ui/badge";
import { cn } from "~/lib/utils";
import { LegacyWorkspace } from "./App";
import { ComingSoonTab } from "./tabs/ComingSoonTab";
import { SettingsTab } from "./tabs/SettingsTab";

type TabId = "servers" | "mods" | "compiler" | "settings";

const TABS: { id: TabId; label: string; icon: ComponentType<{ className?: string }>; soon?: boolean }[] = [
  { id: "servers", label: "Servers", icon: Server },
  { id: "mods", label: "Mods", icon: Puzzle },
  { id: "compiler", label: "Jar → Rust", icon: Binary, soon: true },
  { id: "settings", label: "Settings", icon: SettingsIcon },
];

const TAB_KEY = "moddedmc.tab";

function loadTab(): TabId {
  try {
    const saved = localStorage.getItem(TAB_KEY);
    if (TABS.some((t) => t.id === saved)) return saved as TabId;
  } catch {
    // No storage: start on Servers.
  }
  return "servers";
}

export function Shell() {
  const [tab, setTab] = useState<TabId>(loadTab);

  const select = (next: TabId) => {
    setTab(next);
    try {
      localStorage.setItem(TAB_KEY, next);
    } catch {
      // Remembering the tab is a convenience only.
    }
  };

  return (
    <div className="flex h-full flex-col bg-background text-foreground">
      <header className="flex h-12 shrink-0 items-center gap-3 border-b px-3">
        <div className="flex w-44 items-center gap-2">
          <img src={logo} alt="" className="size-7" draggable={false} />
          <span className="font-semibold text-sm tracking-tight">moddedMC</span>
        </div>
        <nav
          role="tablist"
          aria-label="Sections"
          className="mx-auto flex items-center gap-0.5 rounded-lg bg-muted p-0.5"
        >
          {TABS.map(({ id, label, icon: Icon, soon }) => (
            <button
              key={id}
              role="tab"
              aria-selected={tab === id}
              onClick={() => select(id)}
              className={cn(
                "flex h-8 items-center gap-1.5 rounded-md px-3 font-medium text-sm transition-colors",
                tab === id
                  ? "bg-background text-foreground shadow-xs dark:bg-accent"
                  : "text-muted-foreground hover:text-foreground",
              )}
            >
              <Icon className="size-4" />
              {label}
              {soon && (
                <Badge variant="outline" size="sm">
                  Soon
                </Badge>
              )}
            </button>
          ))}
        </nav>
        <div className="w-44" />
      </header>
      <main className="min-h-0 flex-1">
        {tab === "servers" && (
          <div className="legacy h-full">
            <LegacyWorkspace />
          </div>
        )}
        {tab === "mods" && (
          <div className="legacy h-full">
            <LegacyWorkspace initial={{ kind: "mods" }} />
          </div>
        )}
        {tab === "compiler" && <ComingSoonTab />}
        {tab === "settings" && <SettingsTab />}
      </main>
    </div>
  );
}
