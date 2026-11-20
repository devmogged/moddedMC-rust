import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { SendHorizontal } from "lucide-react";
import { api, type Device, type ServerInfo } from "~/api";
import { Button } from "~/components/ui/button";
import { Input } from "~/components/ui/input";
import { errorText } from "~/lib/hooks";
import { toast } from "~/lib/toast";
import { cn } from "~/lib/utils";

const MAX_LINES = 2000;

export function ConsolePanel({ device, server }: { device: Device; server: ServerInfo }) {
  const [lines, setLines] = useState<string[]>([]);
  const [connected, setConnected] = useState(false);
  const [command, setCommand] = useState("");
  const [history, setHistory] = useState<string[]>([]);
  const [cursor, setCursor] = useState(-1);
  const idRef = useRef<number | null>(null);
  const logRef = useRef<HTMLDivElement>(null);
  const stick = useRef(true);

  useEffect(() => {
    let alive = true;
    let retry: number | undefined;
    const unlisten = api.onConsole(({ id, line }) => {
      if (id !== idRef.current) return;
      if (line === null) {
        idRef.current = null;
        setConnected(false);
        retry = window.setTimeout(open, 2000);
        return;
      }
      setLines((l) => (l.length >= MAX_LINES ? [...l.slice(-MAX_LINES + 1), line] : [...l, line]));
    });
    const open = async () => {
      try {
        const id = await api.consoleOpen(device.id, server.name);
        if (!alive) {
          api.consoleClose(id);
          return;
        }
        // The agent replays its backlog on every connect.
        setLines([]);
        idRef.current = id;
        setConnected(true);
      } catch {
        if (alive) retry = window.setTimeout(open, 3000);
      }
    };
    open();
    return () => {
      alive = false;
      window.clearTimeout(retry);
      unlisten.then((f) => f());
      if (idRef.current !== null) api.consoleClose(idRef.current);
      idRef.current = null;
    };
  }, [device.id, server.name]);

  useLayoutEffect(() => {
    const el = logRef.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [lines]);

  const send = async () => {
    const line = command.trim();
    if (!line || idRef.current === null) return;
    setCommand("");
    setHistory((h) => [line, ...h.filter((x) => x !== line)].slice(0, 50));
    setCursor(-1);
    try {
      await api.consoleSend(idRef.current, line.replace(/^\//, ""));
    } catch (err) {
      toast(errorText(err), "error");
    }
  };

  return (
    <div className="flex h-full flex-col gap-3 p-4">
      <div
        ref={logRef}
        onScroll={(e) => {
          const el = e.currentTarget;
          stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
        }}
        className="min-h-0 flex-1 overflow-y-auto rounded-xl border bg-card p-3 font-mono text-xs leading-relaxed select-text"
      >
        {lines.length === 0 ? (
          <span className="text-muted-foreground">{connected ? "No output yet." : "Connecting…"}</span>
        ) : (
          lines.map((line, i) => (
            <div
              key={i}
              className={cn(
                "whitespace-pre-wrap break-all",
                /\b(ERROR|panicked)\b/.test(line) && "text-destructive-foreground",
                /\bWARN\b/.test(line) && "text-warning-foreground",
              )}
            >
              {line}
            </div>
          ))
        )}
      </div>
      <form
        className="flex gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          send();
        }}
      >
        <Input
          value={command}
          onChange={(e) => setCommand(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "ArrowUp" && history.length) {
              e.preventDefault();
              const n = Math.min(cursor + 1, history.length - 1);
              setCursor(n);
              setCommand(history[n]);
            } else if (e.key === "ArrowDown") {
              e.preventDefault();
              const n = cursor - 1;
              setCursor(Math.max(n, -1));
              setCommand(n >= 0 ? history[n] : "");
            }
          }}
          disabled={!connected || server.state !== "running"}
          placeholder={
            server.state === "running"
              ? "Type a command, like op Steve or say hi"
              : "Start the server to send commands"
          }
          className="font-mono"
        />
        <Button type="submit" variant="outline" disabled={!connected || !command.trim()}>
          <SendHorizontal /> Send
        </Button>
      </form>
    </div>
  );
}
