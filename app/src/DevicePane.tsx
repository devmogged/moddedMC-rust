import { useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import { api, type Device } from "./api";
import type { DeviceData, Selection } from "./App";
import { ServerCluster, ServerIcon, packLine, bytes, routeLabel, usePoll } from "./ui";
import { ConfirmDialog, NewServerDialog } from "./dialogs";

export function DevicePane({
  device,
  data,
  onSelect,
  onChanged,
  onRemoved,
  onError,
}: {
  device: Device;
  data: DeviceData | undefined;
  onSelect: (s: Selection) => void;
  onChanged: () => void;
  onRemoved: () => void;
  onError: (err: unknown) => void;
}) {
  const info = usePoll(() => api.deviceInfo(device.id), 10_000, [device.id]);
  const [dialog, setDialog] = useState<"server" | "remove" | null>(null);
  const servers = data?.servers ?? null;
  const reachError = data?.error ?? (info.value ? null : info.error);
  const i = info.value;

  return (
    <div className="pane">
      <header className="pane-head">
        <div className="pane-head-text">
          <h1 className="pane-title">{device.name}</h1>
          <p className="row-line">
            {routeLabel[device.route]}
            {device.host ? ` · ${device.host}` : ""}
          </p>
        </div>
        <div className="actions">
          {device.route !== "local" ? (
            <button className="button" onClick={() => setDialog("remove")}>
              <Trash2 />
              Remove
            </button>
          ) : null}
          {servers ? (
            <button className="button primary" onClick={() => setDialog("server")}>
              <Plus />
              New server
            </button>
          ) : null}
        </div>
      </header>

      <div className="pane-body ruled">
        {reachError && !i ? (
          <p className="error-line">{reachError}</p>
        ) : (
          <>
            <section className="block">
              <div className="block-head">Servers</div>
              {servers && servers.length === 0 ? (
                <p className="row-line">No servers on {device.name}.</p>
              ) : (
                <div className="list">
                  {servers?.map((server) => (
                    <button
                      key={server.name}
                      className="row"
                      onClick={() => onSelect({ kind: "server", device: device.id, server: server.name })}
                    >
                      <ServerIcon name={server.name} />
                      <span className="row-text">
                        <span className="row-name">{server.name}</span>
                        <span className="row-line">{packLine(server)}</span>
                      </span>
                      <ServerCluster device={device} server={server} />
                    </button>
                  ))}
                </div>
              )}
            </section>

            {i ? (
              <section className="block">
                <div className="block-head">Machine</div>
                <dl className="facts">
                  <dt>System</dt>
                  <dd>
                    {i.os} · {i.arch}
                  </dd>
                  <dt>CPU</dt>
                  <dd>{i.cpus} threads</dd>
                  <dt>Memory</dt>
                  <dd>
                    {bytes(i.ram_available)} free of {bytes(i.ram_total)}
                  </dd>
                  <dt>Disk</dt>
                  <dd>{bytes(i.disk_free)} free</dd>
                  {i.tailscale_name ? (
                    <>
                      <dt>Tailscale</dt>
                      <dd>{i.tailscale_name}</dd>
                    </>
                  ) : null}
                  <dt>Agent</dt>
                  <dd>{i.agent_version}</dd>
                </dl>
              </section>
            ) : null}
          </>
        )}
      </div>

      <NewServerDialog
        open={dialog === "server"}
        device={device}
        onClose={() => setDialog(null)}
        onCreated={(name) => {
          onChanged();
          onSelect({ kind: "server", device: device.id, server: name });
        }}
      />
      <ConfirmDialog
        open={dialog === "remove"}
        title={`Remove ${device.name}?`}
        body="Its servers keep running. You can add it again later."
        action="Remove"
        onClose={() => setDialog(null)}
        onConfirm={async () => {
          try {
            await api.removeDevice(device.id);
            onRemoved();
          } catch (err) {
            onError(err);
          }
        }}
      />
    </div>
  );
}
