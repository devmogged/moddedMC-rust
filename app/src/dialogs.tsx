import { useState, type FormEvent, type ReactNode } from "react";
import { api, type Build, type Device, type Friend, type Route, type ServerInfo } from "./api";
import { Dialog, ServerCluster, ServerIcon, packLine } from "./ui";
import type { DeviceData } from "./App";

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
  // Set once the agent didn't answer: the dialog then offers to install it over SSH.
  const [sshLogin, setSshLogin] = useState<string | null>(null);

  const close = () => {
    setName("");
    setHost("");
    setError(null);
    setSshLogin(null);
    onClose();
  };

  const add = async () => {
    const device = await api.addDevice({ name, route, host });
    // Check the agent answers before closing, so a typo shows here and not later.
    await api.deviceInfo(device.id).catch(async (err) => {
      await api.removeDevice(device.id);
      throw err;
    });
    close();
    onAdded(device);
  };

  return (
    <Dialog open={open} onClose={close}>
      <Form
        title="Add device"
        submit={sshLogin === null ? "Add device" : "Install agent"}
        busyLabel={sshLogin === null ? "Connecting…" : "Installing…"}
        error={error}
        onClose={close}
        onSubmit={async () => {
          try {
            if (sshLogin !== null) {
              await api.installAgent(sshLogin);
            }
            await add();
          } catch (err) {
            setError((err as Error).message);
            setSshLogin((login) => login ?? host.trim());
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
        {sshLogin !== null && route === "tailscale" ? (
          <label className="field">
            <span>SSH login</span>
            <input
              className="input mono"
              value={sshLogin}
              onChange={(e) => setSshLogin(e.target.value)}
              placeholder="pi@pi"
              spellCheck={false}
              required
            />
          </label>
        ) : null}
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

export function DeployDialog({
  open,
  device,
  server,
  onClose,
  onDeployed,
}: {
  open: boolean;
  device: Device;
  server: string;
  onClose: () => void;
  onDeployed: (build: Build) => void;
}) {
  const [build, setBuild] = useState<Build | null>(null);
  const [error, setError] = useState<string | null>(null);

  const close = () => {
    setBuild(null);
    setError(null);
    onClose();
  };

  const choose = async () => {
    try {
      const path = await api.pickFolder();
      if (!path) return;
      setBuild(await api.inspectBuild(path));
      setError(null);
    } catch (err) {
      setBuild(null);
      setError((err as Error).message);
    }
  };

  return (
    <Dialog open={open} onClose={close}>
      <Form
        title={`Deploy to ${server}`}
        submit="Deploy"
        busyLabel="Uploading…"
        error={error}
        onClose={close}
        onSubmit={async () => {
          if (!build) {
            setError("Choose a build folder first.");
            return;
          }
          try {
            const deployed = await api.deploy(device.id, server, build.path);
            close();
            onDeployed(deployed);
          } catch (err) {
            setError((err as Error).message);
          }
        }}
      >
        <div className="field">
          <span>Build</span>
          <button type="button" className="build-pick" onClick={choose}>
            {build ? (
              <>
                <span className="row-name">
                  {build.pack} {build.version}
                </span>
                <span className="row-line mono">{build.path}</span>
              </>
            ) : (
              <span className="row-line">Choose build folder</span>
            )}
          </button>
        </div>
        <p className="row-line" style={{ fontSize: 13 }}>
          The server's drop-in mods are replaced with the build's. The world stays.
        </p>
      </Form>
    </Dialog>
  );
}

/** Deploy a build folder to any stopped server the owner manages. */
export function DeployToDialog({
  open,
  buildPath,
  devices,
  data,
  onClose,
  onDeployed,
}: {
  open: boolean;
  buildPath: string;
  devices: Device[];
  data: Record<string, DeviceData>;
  onClose: () => void;
  onDeployed: () => void;
}) {
  const [choice, setChoice] = useState<{ device: Device; server: ServerInfo } | null>(null);
  const [error, setError] = useState<string | null>(null);

  const close = () => {
    setChoice(null);
    setError(null);
    onClose();
  };

  const targets = devices.flatMap((device) =>
    (data[device.id]?.servers ?? []).filter((s) => !s.external).map((server) => ({ device, server })),
  );

  return (
    <Dialog open={open} onClose={close}>
      <Form
        title="Deploy to a server"
        submit="Deploy"
        busyLabel="Uploading…"
        error={error}
        onClose={close}
        onSubmit={async () => {
          if (!choice) {
            setError("Choose a server first.");
            return;
          }
          try {
            await api.deploy(choice.device.id, choice.server.name, buildPath);
            close();
            onDeployed();
          } catch (err) {
            setError((err as Error).message);
          }
        }}
      >
        {targets.length === 0 ? (
          <p className="row-line">No servers yet. Create one on a device first.</p>
        ) : (
          <div className="list pick-list" role="radiogroup" aria-label="Server">
            {targets.map(({ device, server }) => {
              const busy =
                server.state === "running" || server.state === "starting" || server.state === "stopping";
              const picked = choice?.device.id === device.id && choice.server.name === server.name;
              return (
                <button
                  key={`${device.id}/${server.name}`}
                  type="button"
                  className="row"
                  role="radio"
                  aria-checked={picked}
                  aria-current={picked}
                  disabled={busy}
                  title={busy ? `Stop ${server.name} first` : undefined}
                  onClick={() => setChoice({ device, server })}
                >
                  <ServerIcon name={server.name} />
                  <span className="row-text">
                    <span className="row-name">{server.name}</span>
                    <span className="row-line">
                      {device.name} · {packLine(server)}
                    </span>
                  </span>
                  <ServerCluster device={device} server={server} />
                </button>
              );
            })}
          </div>
        )}
        <p className="row-line" style={{ fontSize: 13 }}>
          The server's drop-in mods are replaced with the build's. The world stays.
        </p>
      </Form>
    </Dialog>
  );
}
