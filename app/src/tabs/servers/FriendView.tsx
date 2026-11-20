import { useState } from "react";
import { LogIn, Trash2 } from "lucide-react";
import { api, type Friend } from "~/api";
import { Button } from "~/components/ui/button";
import {
  Dialog,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogPanel,
  DialogPopup,
  DialogTitle,
} from "~/components/ui/dialog";
import { Input } from "~/components/ui/input";
import { errorText, usePoll } from "~/lib/hooks";
import { toast } from "~/lib/toast";

/** A friend's server: whether it's up, and joining it through the player's launcher. */
export function FriendView({ friend, onRemoved }: { friend: Friend; onRemoved: () => void }) {
  const status = usePoll(() => api.ping(friend.address), 10_000, [friend.address]);
  const [busy, setBusy] = useState(false);
  const s = status.value;

  const join = async () => {
    setBusy(true);
    try {
      toast(await api.joinFriend(friend.id));
    } catch (err) {
      toast(errorText(err), "error");
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    try {
      await api.removeFriend(friend.id);
      onRemoved();
    } catch (err) {
      toast(errorText(err), "error");
    }
  };

  return (
    <div className="flex flex-col gap-6 p-6">
      <div className="flex items-center gap-4">
        {s?.favicon ? (
          <img src={s.favicon} alt="" className="size-16 rounded-lg [image-rendering:pixelated]" />
        ) : (
          <div className="size-16 rounded-lg border border-dashed" />
        )}
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <h1 className="truncate font-semibold text-xl">{friend.name}</h1>
          <span className="text-muted-foreground text-sm">{friend.address}</span>
        </div>
        <Button disabled={busy || !s} onClick={join}>
          <LogIn /> Join
        </Button>
        <Button size="icon" variant="ghost-destructive" aria-label="Remove" onClick={remove}>
          <Trash2 />
        </Button>
      </div>
      <div className="rounded-xl border bg-card p-4 text-sm">
        {s ? (
          <div className="flex flex-col gap-1">
            <span>{s.motd}</span>
            <span className="text-muted-foreground">
              {s.players_online ?? "?"} / {s.players_max ?? "?"} players · {s.latency_ms} ms
              {s.version ? ` · ${s.version}` : ""}
            </span>
          </div>
        ) : (
          <span className="text-muted-foreground">
            {status.error ? "Offline or unreachable." : "Checking…"}
          </span>
        )}
      </div>
    </div>
  );
}

export function AddFriendDialog({
  open,
  onOpenChange,
  onAdded,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onAdded: (id: string) => void;
}) {
  const [name, setName] = useState("");
  const [address, setAddress] = useState("");
  const add = async () => {
    try {
      const friend = await api.addFriend({ name: name.trim(), address: address.trim() });
      onAdded(friend.id);
      onOpenChange(false);
      setName("");
      setAddress("");
    } catch (err) {
      toast(errorText(err), "error");
    }
  };
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogPopup className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Add a friend's server</DialogTitle>
          <DialogDescription>moddedMC checks it's up and sets up your launcher to join it.</DialogDescription>
        </DialogHeader>
        <DialogPanel className="flex flex-col gap-3">
          <Input
            autoFocus
            placeholder="Name, like Sam's world"
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
          <Input
            placeholder="Address, like play.example.com:25565"
            value={address}
            onChange={(e) => setAddress(e.target.value)}
          />
        </DialogPanel>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button disabled={!name.trim() || !address.trim()} onClick={add}>
            Add
          </Button>
        </DialogFooter>
      </DialogPopup>
    </Dialog>
  );
}
