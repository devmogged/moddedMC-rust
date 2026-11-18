import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Hammer, Trash2, Upload } from "lucide-react";
import { api, type Device, type Pack } from "./api";
import type { DeviceData } from "./App";
import { ConfirmDialog, DeployToDialog } from "./dialogs";

const MAX_LOG = 400;

export function PackPane({
  pack,
  devices,
  data,
  onChanged,
  onRemoved,
  onError,
}: {
  pack: Pack;
  devices: Device[];
  data: Record<string, DeviceData>;
  onChanged: () => void;
  onRemoved: () => void;
  onError: (err: unknown) => void;
}) {
  const [building, setBuilding] = useState<string | null>(null);
  const [log, setLog] = useState<string[]>([]);
  const [deployFrom, setDeployFrom] = useState<string | null>(null);
  const [confirm, setConfirm] = useState(false);
  const logRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const unlisten = api.onPackBuild(({ path, line }) => {
      if (path !== pack.path) return;
      setLog((prev) => [...prev.slice(-(MAX_LOG - 1)), line]);
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, [pack.path]);

  useLayoutEffect(() => {
    const el = logRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [log]);

  const build = async (target: string) => {
    setBuilding(target);
    setLog([]);
    try {
      await api.buildPack(pack.path, target);
      onChanged();
    } catch (err) {
      setLog((prev) => [...prev, (err as Error).message]);
      onError(err);
    } finally {
      setBuilding(null);
    }
  };

  const line = [pack.version ? `Version ${pack.version}` : null, `${pack.mods.length} mods`, pack.path]
    .filter(Boolean)
    .join(" · ");

  return (
    <div className="pane">
      <header className="pane-head">
        <div className="pane-head-text">
          <h1 className="pane-title">{pack.name}</h1>
          <p className="row-line">{line}</p>
        </div>
        <div className="actions">
          <button className="button" onClick={() => setConfirm(true)}>
            <Trash2 />
            Remove
          </button>
        </div>
      </header>

      <div className="pane-body ruled">
        {pack.error ? <p className="error-line">{pack.error}</p> : null}

        <section className="block">
          <div className="block-head">Targets</div>
          <div>
            {pack.targets.map((t) => (
              <div key={t.name} className="target-row">
                <div className="row-text">
                  <div className="row-name">{t.name}</div>
                  <div className="row-line mono">{t.triple}</div>
                </div>
                <span className="row-line">
                  {building === t.name ? "Building…" : t.built ? "Built" : "Not built"}
                </span>
                <div className="actions">
                  <button
                    className={t.built ? "button" : "button primary"}
                    disabled={building !== null || pack.error !== null}
                    onClick={() => build(t.name)}
                  >
                    <Hammer />
                    {t.built ? "Rebuild" : "Build"}
                  </button>
                  {t.built ? (
                    <button
                      className="button primary"
                      disabled={building !== null}
                      onClick={() => setDeployFrom(t.built)}
                    >
                      <Upload />
                      Deploy
                    </button>
                  ) : null}
                </div>
              </div>
            ))}
          </div>
        </section>

        {log.length > 0 ? (
          <section className="block">
            <div className="block-head">Build output</div>
            <div className="build-log" ref={logRef} role="log">
              {log.map((l, i) => (
                <div key={i}>{l}</div>
              ))}
            </div>
          </section>
        ) : null}

        <section className="block">
          <div className="block-head">Mods</div>
          <div>
            {pack.mods.map((m) => (
              <div key={m.id} className="mod-row">
                <div className="row-name">{m.id}</div>
                <span />
                <span className="tag">
                  {m.source === "index" ? "Published" : m.source === "mmc" ? "File" : "Local"}
                </span>
              </div>
            ))}
          </div>
        </section>
      </div>

      <DeployToDialog
        open={deployFrom !== null}
        buildPath={deployFrom ?? ""}
        devices={devices}
        data={data}
        onClose={() => setDeployFrom(null)}
        onDeployed={onChanged}
      />
      <ConfirmDialog
        open={confirm}
        title={`Remove ${pack.name}?`}
        body="It leaves this list. The pack file and its builds stay on disk."
        action="Remove"
        onClose={() => setConfirm(false)}
        onConfirm={async () => {
          try {
            await api.removePack(pack.path);
            onRemoved();
          } catch (err) {
            onError(err);
          }
        }}
      />
    </div>
  );
}
