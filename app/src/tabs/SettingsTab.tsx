import { useEffect, useState, type ReactNode } from "react";
import { Monitor, Moon, Sun } from "lucide-react";
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
