import { useEffect, useId, useRef, useState } from "react";
import { ArrowDown, ArrowUp, CornerDownLeft, Search, X, type LucideIcon } from "lucide-react";
import { Dialog } from "./ui";

export type Command = {
  id: string;
  label: string;
  context?: string;
  icon: LucideIcon;
  reason?: string;
  run: () => void;
};

export function CommandPalette({
  open,
  close,
  commands,
  onRun,
}: {
  open: boolean;
  close: () => void;
  commands: Command[];
  onRun: (command: Command) => void;
}) {
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const id = useId();
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const words = query.toLocaleLowerCase().trim().split(/\s+/).filter(Boolean);
  const results = commands
    .flatMap((command) => {
      const text = [command.label, command.context].join(" ").toLocaleLowerCase();
      if (!words.every((word) => text.includes(word))) return [];
      const tokens = text.split(/[^\p{L}\p{N}]+/u);
      return [{ command, score: words.filter((word) => tokens.includes(word)).length }];
    })
    .sort((a, b) => b.score - a.score)
    .map(({ command }) => command);
  const active = Math.min(index, results.length - 1);

  useEffect(() => {
    if (open) input.current?.focus();
    else {
      setQuery("");
      setIndex(0);
    }
  }, [open]);

  useEffect(() => {
    if (open && active >= 0) list.current?.children[active]?.scrollIntoView({ block: "nearest" });
  }, [open, active]);

  return (
    <Dialog open={open} onClose={close} label="Command palette" className="palette">
      <div className="palette-field">
        <Search aria-hidden />
        <input
          ref={input}
          role="combobox"
          aria-label="Search commands"
          aria-autocomplete="list"
          aria-controls={id}
          aria-expanded={open}
          aria-activedescendant={active >= 0 ? id + "-" + active : undefined}
          value={query}
          placeholder="Search commands"
          autoComplete="off"
          spellCheck={false}
          onChange={(event) => {
            setQuery(event.target.value);
            setIndex(0);
          }}
          onKeyDown={(event) => {
            if (event.nativeEvent.isComposing || !results.length) return;
            if (event.key === "ArrowDown" || event.key === "ArrowUp") {
              event.preventDefault();
              setIndex((active + results.length + (event.key === "ArrowDown" ? 1 : -1)) % results.length);
            } else if (event.key === "Enter") {
              event.preventDefault();
              const command = results[active];
              if (!command.reason) onRun(command);
            }
          }}
        />
        <button
          className="icon-button"
          type="button"
          onClick={close}
          title="Close"
          aria-label="Close command palette"
        >
          <X />
        </button>
      </div>
      <div className="palette-list" id={id} role="listbox" aria-label="Commands" ref={list}>
        {results.map((command, i) => (
          <div
            key={command.id}
            id={id + "-" + i}
            className="palette-option"
            role="option"
            aria-selected={i === active}
            aria-disabled={Boolean(command.reason)}
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => {
              if (!command.reason) onRun(command);
            }}
            onMouseMove={() => setIndex(i)}
          >
            <command.icon aria-hidden />
            <span>{command.label}</span>
            <small>
              {command.context}
              {command.reason && <span className="palette-reason">{command.reason}</span>}
            </small>
          </div>
        ))}
      </div>
      {!results.length && (
        <p className="palette-empty" role="status">
          No matching commands
        </p>
      )}
      <div className="palette-keys" aria-hidden>
        <kbd>
          <ArrowUp />
          <ArrowDown />
        </kbd>{" "}
        Navigate
        <kbd>
          <CornerDownLeft />
        </kbd>{" "}
        Select
        <kbd>Esc</kbd> Close
      </div>
    </Dialog>
  );
}
