import { useState, type FormEvent, type ReactNode } from "react";
import { api, type Device, type Friend, type Route } from "./api";
import { Dialog } from "./ui";

function Form({
  title,
  submit,
  busyLabel,
  onSubmit,
  onClose,
  error,
  danger,
  children,
}: {
  title: string;
  submit: string;
  busyLabel: string;
  onSubmit: () => Promise<void>;
  onClose: () => void;
  error: string | null;
  danger?: boolean;
  children: ReactNode;
}) {
  const [busy, setBusy] = useState(false);
  const handle = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    try {
      await onSubmit();
    } finally {
      setBusy(false);
    }
  };
  return (
    <form className="dialog-form" onSubmit={handle}>
      <h2 className="dialog-title">{title}</h2>
      {children}
      {error ? <p className="error-line">{error}</p> : null}
      <div className="dialog-actions">
        <button type="button" className="button" onClick={onClose}>
          Cancel
        </button>
        <button type="submit" className={danger ? "button danger" : "button primary"} disabled={busy}>
          {busy ? busyLabel : submit}
        </button>
      </div>
    </form>
  );
}

export function AddDeviceDialog({
  open,
  onClose,
  onAdded,
}: {
  open: boolean;
  onClose: () => void;
  onAdded: (device: Device) => void;
}) {
  const [route, setRoute] = useState<Exclude<Route, "local">>("tailscale");
  const [name, setName] = useState("");
  const [host, setHost] = useState("");
  const [error, setError] = useState<string | null>(null);

  const close = () => {
    setName("");
    setHost("");
    setError(null);
    onClose();
  };

  return (
    <Dialog open={open} onClose={close}>
      <Form
        title="Add device"
        submit="Add device"
        busyLabel="Connecting…"
        error={error}
        onClose={close}
        onSubmit={async () => {
          try {
            const device = await api.addDevice({ name, route, host });
            // Check the agent answers before closing, so a typo shows here and not later.
            await api.deviceInfo(device.id).catch(async (err) => {
              await api.removeDevice(device.id);
              throw err;
            });
            close();
            onAdded(device);
          } catch (err) {
            setError((err as Error).message);
          }
        }}
      >
        <div className="segmented" role="group" aria-label="Connection">
          {(["tailscale", "ssh"] as const).map((r) => (
            <button key={r} type="button" aria-pressed={route === r} onClick={() => setRoute(r)}>
              {r === "tailscale" ? "Tailscale" : "SSH"}
            </button>
          ))}
        </div>
        <label className="field">
          <span>Name</span>
          <input
            className="input"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Pi"
            autoFocus
            required
          />
        </label>
        <label className="field">
          <span>{route === "tailscale" ? "Tailscale name" : "SSH destination"}</span>
          <input
            className="input mono"
            value={host}
            onChange={(e) => setHost(e.target.value)}
            placeholder={route === "tailscale" ? "pi" : "user@192.168.1.20"}
            spellCheck={false}
            required
          />
        </label>
      </Form>
    </Dialog>
  );
}

export function AddFriendDialog({
  open,
  onClose,
  onAdded,
}: {
  open: boolean;
  onClose: () => void;
  onAdded: (friend: Friend) => void;
}) {
  const [name, setName] = useState("");
  const [address, setAddress] = useState("");
  const [error, setError] = useState<string | null>(null);

  const close = () => {
    setName("");
    setAddress("");
    setError(null);
    onClose();
  };

  return (
    <Dialog open={open} onClose={close}>
      <Form
        title="Add server"
        submit="Add server"
        busyLabel="Adding…"
        error={error}
        onClose={close}
        onSubmit={async () => {
          try {
            const friend = await api.addFriend({ name, address });
            close();
            onAdded(friend);
          } catch (err) {
            setError((err as Error).message);
          }
        }}
      >
        <label className="field">
          <span>Address</span>
          <input
            className="input mono"
            value={address}
            onChange={(e) => setAddress(e.target.value)}
            placeholder="play.example.net"
            spellCheck={false}
            autoFocus
            required
          />
        </label>
        <label className="field">
          <span>Name</span>
          <input className="input" value={name} onChange={(e) => setName(e.target.value)} />
        </label>
      </Form>
    </Dialog>
  );
}

export function NewServerDialog({
  open,
  device,
  onClose,
  onCreated,
}: {
  open: boolean;
  device: Device;
  onClose: () => void;
  onCreated: (name: string) => void;
}) {
  const [name, setName] = useState("");
  const [exe, setExe] = useState("");
  const [port, setPort] = useState("25565");
  const [error, setError] = useState<string | null>(null);

  const close = () => {
    setName("");
    setExe("");
    setPort("25565");
    setError(null);
    onClose();
  };

  return (
    <Dialog open={open} onClose={close}>
      <Form
        title={`New server on ${device.name}`}
        submit="Create server"
        busyLabel="Creating…"
        error={error}
        onClose={close}
        onSubmit={async () => {
          const portNumber = Number(port);
          if (!Number.isInteger(portNumber) || portNumber < 1 || portNumber > 65535) {
            setError("The port must be a number from 1 to 65535.");
            return;
          }
          try {
            await api.createServer(device.id, { name: name.trim(), exe: exe.trim(), port: portNumber });
            const created = name.trim();
            close();
            onCreated(created);
          } catch (err) {
            setError((err as Error).message);
          }
        }}
      >
        <label className="field">
          <span>Name</span>
          <input
            className="input"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="survival"
            pattern="[A-Za-z0-9._\-]+"
            title="Letters, digits, dots, dashes and underscores"
            autoFocus
            required
          />
        </label>
        <label className="field">
          <span>Server program</span>
          <input
            className="input mono"
            value={exe}
            onChange={(e) => setExe(e.target.value)}
            placeholder="/home/pi/packs/buja/pack-buja"
            spellCheck={false}
            required
          />
        </label>
        <label className="field">
          <span>Port</span>
          <input
            className="input mono"
            value={port}
            onChange={(e) => setPort(e.target.value)}
            inputMode="numeric"
            required
          />
        </label>
      </Form>
    </Dialog>
  );
}

export function ConfirmDialog({
  open,
  title,
  body,
  action,
  onClose,
  onConfirm,
}: {
  open: boolean;
  title: string;
  body: string;
  action: string;
  onClose: () => void;
  onConfirm: () => Promise<void>;
}) {
  return (
    <Dialog open={open} onClose={onClose}>
      <Form
        title={title}
        submit={action}
        busyLabel={`${action}…`}
        error={null}
        danger
        onClose={onClose}
        onSubmit={async () => {
          await onConfirm();
          onClose();
        }}
      >
        <p className="row-line" style={{ fontSize: 13 }}>
          {body}
        </p>
      </Form>
    </Dialog>
  );
}
