import { useEffect, useState } from "react";
import { Plus, Server as ServerIcon, UserPlus, Users } from "lucide-react";
import { api, type Device, type Friend, type ServerInfo } from "~/api";
import { Button } from "~/components/ui/button";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "~/components/ui/empty";
import { Spinner } from "~/components/ui/spinner";
import { usePoll } from "~/lib/hooks";
import { cn } from "~/lib/utils";
import { AddFriendDialog, FriendView } from "./FriendView";
import { NewServerDialog } from "./NewServerDialog";
import { ServerView } from "./ServerView";
import { StateDot } from "./StateDot";

export type Selected = { kind: "server"; device: string; server: string } | { kind: "friend"; id: string };

const SELECTED_KEY = "moddedmc.server";

function loadSelected(): Selected | null {
  try {
    return JSON.parse(localStorage.getItem(SELECTED_KEY) ?? "null");
  } catch {
    return null;
  }
}

export function ServersTab() {
  const devices = usePoll(api.devices, 30_000, []);
  const friends = usePoll(api.friends, 60_000, []);
  const [servers, setServers] = useState<Record<string, { list: ServerInfo[] | null; error: string | null }>>(
    {},
  );
  const [revision, setRevision] = useState(0);
  const refresh = () => setRevision((current) => current + 1);
  const [selected, setSelected] = useState<Selected | null>(loadSelected);
  const [creating, setCreating] = useState(false);
  const [addingFriend, setAddingFriend] = useState(false);

  // Each device's servers, every few seconds, so states and player counts stay live.
  const deviceIds = (devices.value ?? []).map((d) => d.id).join(",");
  useEffect(() => {
    let alive = true;
    const timers = new Map<string, number>();
    for (const device of devices.value ?? []) {
      const run = async () => {
        try {
          const list = await api.servers(device.id);
          if (alive) setServers((s) => ({ ...s, [device.id]: { list, error: null } }));
        } catch (err) {
          if (alive)
            setServers((s) => ({
              ...s,
              [device.id]: { list: s[device.id]?.list ?? null, error: String(err) },
            }));
        }
        if (alive) timers.set(device.id, window.setTimeout(run, 3000));
      };
      run();
    }
    return () => {
      alive = false;
      timers.forEach(window.clearTimeout);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [deviceIds, revision]);

  useEffect(() => {
    try {
      localStorage.setItem(SELECTED_KEY, JSON.stringify(selected));
    } catch {
      // Remembering the selection is a convenience only.
    }
  }, [selected]);

  const all = (devices.value ?? []).flatMap((d) =>
    (servers[d.id]?.list ?? []).map((s) => ({ device: d, server: s })),
  );
  const current =
    selected?.kind === "server"
      ? all.find((x) => x.device.id === selected.device && x.server.name === selected.server)
      : undefined;
  const friend = selected?.kind === "friend" ? friends.value?.find((f) => f.id === selected.id) : undefined;
  const loading = !devices.value || (devices.value.length > 0 && Object.keys(servers).length === 0);

  return (
    <div className="flex h-full">
      <aside className="flex w-64 shrink-0 flex-col border-r">
        <div className="flex h-12 items-center justify-between px-3">
          <span className="font-medium text-sm">Servers</span>
          <Button size="xs" variant="outline" onClick={() => setCreating(true)}>
            <Plus /> New
          </Button>
        </div>
        <div className="flex-1 overflow-y-auto px-2 pb-3">
          {(devices.value ?? []).map((device) => (
            <DeviceGroup
              key={device.id}
              device={device}
              state={servers[device.id]}
              selected={selected}
              onSelect={(server) => setSelected({ kind: "server", device: device.id, server })}
            />
          ))}
          <FriendGroup
            friends={friends.value ?? []}
            selected={selected}
            onSelect={(id) => setSelected({ kind: "friend", id })}
          />
        </div>
        <div className="border-t p-2">
          <Button
            variant="ghost-muted"
            size="sm"
            className="w-full justify-start"
            onClick={() => setAddingFriend(true)}
          >
            <UserPlus /> Join a friend's server
          </Button>
        </div>
      </aside>
      <section className="min-w-0 flex-1">
        {current ? (
          <ServerView
            key={`${current.device.id}/${current.server.name}`}
            device={current.device}
            server={current.server}
            onChanged={refresh}
            onPortSaved={(port) => {
              const { id } = current.device;
              setServers((data) => ({
                ...data,
                [id]: {
                  ...data[id],
                  list:
                    data[id].list?.map((server) =>
                      server.name === current.server.name ? { ...server, port, start_error: null } : server,
                    ) ?? null,
                },
              }));
              refresh();
            }}
            onDeleted={() => {
              const { id } = current.device;
              const { name } = current.server;
              setServers((data) => ({
                ...data,
                [id]: { ...data[id], list: data[id].list?.filter((server) => server.name !== name) ?? null },
              }));
              setSelected((selection) =>
                selection?.kind === "server" && selection.device === id && selection.server === name
                  ? null
                  : selection,
              );
              refresh();
            }}
          />
        ) : friend ? (
          <FriendView key={friend.id} friend={friend} onRemoved={() => friends.refresh()} />
        ) : loading ? (
          <div className="flex h-full items-center justify-center">
            <Spinner />
          </div>
        ) : (
          <Empty className="h-full">
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <ServerIcon />
              </EmptyMedia>
              <EmptyTitle>{all.length ? "Pick a server" : "Make your first server"}</EmptyTitle>
              <EmptyDescription>
                {all.length
                  ? "Choose one on the left to start it, read its console or change its settings."
                  : "Set up a modded Minecraft server on this PC in a minute, then invite your friends."}
              </EmptyDescription>
            </EmptyHeader>
            <Button onClick={() => setCreating(true)}>
              <Plus /> New server
            </Button>
          </Empty>
        )}
      </section>
      <AddFriendDialog
        open={addingFriend}
        onOpenChange={setAddingFriend}
        onAdded={(id) => {
          friends.refresh();
          setSelected({ kind: "friend", id });
        }}
      />
      <NewServerDialog
        open={creating}
        onOpenChange={setCreating}
        onCreated={(device, server) => {
          setSelected({ kind: "server", device, server });
          refresh();
        }}
      />
    </div>
  );
}

function DeviceGroup({
  device,
  state,
  selected,
  onSelect,
}: {
  device: Device;
  state?: { list: ServerInfo[] | null; error: string | null };
  selected: Selected | null;
  onSelect: (server: string) => void;
}) {
  return (
    <div className="mt-2">
      <div className="flex items-center gap-2 px-2 py-1 text-muted-foreground text-xs">
        <span className="truncate font-medium">{device.name}</span>
        {state?.error && <span className="text-destructive-foreground">offline</span>}
      </div>
      {state?.list?.length === 0 && (
        <p className="px-2 py-1 text-muted-foreground text-xs">No servers yet.</p>
      )}
      {state?.list?.map((server) => {
        const active =
          selected?.kind === "server" && selected.device === device.id && selected.server === server.name;
        return (
          <button
            key={server.name}
            onClick={() => onSelect(server.name)}
            className={cn(
              "flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left text-sm transition-colors",
              active
                ? "bg-accent text-foreground"
                : "text-muted-foreground hover:bg-accent/60 hover:text-foreground",
            )}
          >
            <StateDot state={server.state} />
            <span className="flex-1 truncate">{server.name}</span>
            {server.health && (
              <span className="text-muted-foreground text-xs tabular-nums">
                {server.health.players}/{server.health.max_players}
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}

function FriendGroup({
  friends,
  selected,
  onSelect,
}: {
  friends: Friend[];
  selected: Selected | null;
  onSelect: (id: string) => void;
}) {
  if (!friends.length) return null;
  return (
    <div className="mt-4">
      <div className="flex items-center gap-2 px-2 py-1 font-medium text-muted-foreground text-xs">
        <Users className="size-3.5" /> Friends' servers
      </div>
      {friends.map((f) => (
        <button
          key={f.id}
          onClick={() => onSelect(f.id)}
          className={cn(
            "flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left text-sm transition-colors",
            selected?.kind === "friend" && selected.id === f.id
              ? "bg-accent text-foreground"
              : "text-muted-foreground hover:bg-accent/60 hover:text-foreground",
          )}
        >
          <span className="flex-1 truncate">{f.name}</span>
        </button>
      ))}
    </div>
  );
}
