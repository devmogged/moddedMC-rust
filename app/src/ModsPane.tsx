import { api } from "./api";
import { usePoll } from "./ui";

export function ModsPane() {
  const { value, error, refresh } = usePoll(api.mods, 10 * 60_000, []);

  return (
    <div className="pane">
      <header className="pane-head">
        <div className="pane-head-text">
          <h1 className="pane-title">Mods</h1>
        </div>
      </header>
      <div className="pane-body ruled">
        {error && !value ? (
          <div className="empty">
            <div className="stack">
              <p>{error}</p>
              <button className="button" onClick={refresh}>
                Retry
              </button>
            </div>
          </div>
        ) : value && value.length === 0 ? (
          <div className="empty">
            <p>No ported mods yet.</p>
          </div>
        ) : (
          <div>
            {value?.map((m) => (
              <div key={m.id} className="mod-row">
                <div className="row-text">
                  <div className="row-name">{m.name}</div>
                  <div className="row-line">
                    {m.version}
                    {m.minecraft ? ` · Minecraft ${m.minecraft}` : ""}
                  </div>
                </div>
                <span className="tag">{m.kind === "port" ? "Compiled in" : "Drop-in"}</span>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
