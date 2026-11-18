import { useState } from "react";
import { Copy, Trash2 } from "lucide-react";
import { api, type Friend, type Status } from "./api";
import { Bars, ServerIcon, pingBars, usePoll } from "./ui";
import { ConfirmDialog } from "./dialogs";

export interface FriendStatus {
  status: Status | null;
  error: string | null;
}

export function useFriendStatus(address: string): FriendStatus {
  const { value, error } = usePoll(() => api.ping(address), 10_000, [address]);
  return { status: error ? null : value, error };
}

function FriendCluster({ status, error }: FriendStatus) {
  if (error) {
    return (
      <span className="cluster">
        <span className="word">Offline</span>
      </span>
    );
  }
  if (!status) {
    return (
      <span className="cluster">
        <Bars lit={0} tone="starting" label="Pinging" />
      </span>
    );
  }
  return (
    <span className="cluster">
      <Bars lit={pingBars(status.latency_ms)} tone="ok" label={`${status.latency_ms} ms`} />
      {status.players_online != null && status.players_max != null ? (
        <span>
          {status.players_online}/{status.players_max}
        </span>
      ) : null}
    </span>
  );
}

export function FriendRow({
  friend,
  status,
  current,
  onSelect,
}: {
  friend: Friend;
  status: FriendStatus;
  current: boolean;
  onSelect: () => void;
}) {
  return (
    <button className="row" aria-current={current} onClick={onSelect}>
      <ServerIcon name={friend.address} favicon={status.status?.favicon} />
      <span className="row-text">
        <span className="row-name">{friend.name}</span>
        <span className="row-line">{status.status?.motd || friend.address}</span>
      </span>
      <FriendCluster {...status} />
    </button>
  );
}

export function FriendPane({
  friend,
  onRemoved,
  onError,
}: {
  friend: Friend;
  onRemoved: () => void;
  onError: (err: unknown) => void;
}) {
  const live = useFriendStatus(friend.address);
  const [confirm, setConfirm] = useState(false);
  const [copied, setCopied] = useState(false);
  const s = live.status;

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(friend.address);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch (err) {
      onError(err);
    }
  };

  return (
    <div className="pane">
      <header className="pane-head">
        <div className="row large">
          <ServerIcon name={friend.address} favicon={s?.favicon} />
          <div className="row-text">
            <h1 className="row-name">{friend.name}</h1>
            <p className="row-line">{s?.motd || friend.address}</p>
          </div>
          <FriendCluster {...live} />
        </div>
        <div className="actions">
          <button className="button" onClick={() => setConfirm(true)}>
            <Trash2 />
            Remove
          </button>
          <button className="button primary" onClick={copy}>
            <Copy />
            {copied ? "Copied" : "Copy address"}
          </button>
        </div>
      </header>
      <div className="pane-body ruled">
        {live.error && !s ? <p className="error-line">{live.error}</p> : null}
        <dl className="facts">
          <dt>Address</dt>
          <dd>{friend.address}</dd>
          {s?.version ? (
            <>
              <dt>Version</dt>
              <dd>{s.version}</dd>
            </>
          ) : null}
          {s ? (
            <>
              <dt>Ping</dt>
              <dd>{s.latency_ms} ms</dd>
            </>
          ) : null}
        </dl>
      </div>
      <ConfirmDialog
        open={confirm}
        title={`Remove ${friend.name}?`}
        body="It leaves your Join list. Nothing on the server changes."
        action="Remove"
        onClose={() => setConfirm(false)}
        onConfirm={async () => {
          try {
            await api.removeFriend(friend.id);
            onRemoved();
          } catch (err) {
            onError(err);
          }
        }}
      />
    </div>
  );
}
