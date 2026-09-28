import * as Dialog from "@radix-ui/react-dialog";
import { type KeyboardEvent, useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { routes } from "../../routes/routes";
import { Button, Kbd } from "../ui";

interface CommandPaletteProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

interface CommandItem {
  id: string;
  group: "Routes" | "Actions";
  label: string;
  description: string;
  shortcut?: string;
  run: () => void;
}

function optionId(id: string) {
  return `command-option-${id.replace(/[^a-z0-9_-]/gi, "-")}`;
}

export function CommandPalette({ open, onOpenChange }: CommandPaletteProps) {
  const navigate = useNavigate();
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  const commands = useMemo<CommandItem[]>(
    () => [
      ...routes
        .filter((route) => route.command !== false)
        .map((route) => ({
          id: `route:${route.path}`,
          group: "Routes" as const,
          label: route.title,
          description: route.description,
          shortcut: route.shortcut,
          run: () => {
            navigate(route.path);
            onOpenChange(false);
          },
        })),
      {
        id: "action:pause",
        group: "Actions",
        label: "Pause for 15 minutes",
        description: "Stop recognition temporarily from this Mac.",
        shortcut: "⌥⌘F",
        run: () => onOpenChange(false),
      },
      {
        id: "action:teach",
        group: "Actions",
        label: "Teach Ventilador dormitorio",
        description: "Open the point-to-select teach flow for the owner fan scenario.",
        shortcut: "T",
        run: () => {
          navigate("/devices/teach");
          onOpenChange(false);
        },
      },
      {
        id: "action:mapping",
        group: "Actions",
        label: "Add mapping",
        description: "Create a global or targeted gesture sentence.",
        shortcut: "M",
        run: () => {
          navigate("/mappings/new");
          onOpenChange(false);
        },
      },
    ],
    [navigate, onOpenChange],
  );

  const normalizedQuery = query.trim().toLowerCase();
  const filtered = normalizedQuery
    ? commands
        .map((command, index) => {
          const label = command.label.toLowerCase();
          const description = command.description.toLowerCase();
          const rank = label.startsWith(normalizedQuery)
            ? 0
            : label.includes(normalizedQuery)
              ? 1
              : description.includes(normalizedQuery)
                ? 2
                : 3;
          return { command, index, rank };
        })
        .filter((item) => item.rank < 3)
        .sort((a, b) => a.rank - b.rank || a.index - b.index)
        .map((item) => item.command)
    : commands;

  useEffect(() => {
    if (open) {
      setQuery("");
      setActive(0);
      window.setTimeout(() => inputRef.current?.focus(), 0);
    }
  }, [open]);

  const grouped = filtered.reduce<Record<string, CommandItem[]>>((acc, command) => {
    acc[command.group] = [...(acc[command.group] ?? []), command];
    return acc;
  }, {});
  const activeOptionId = filtered[active] ? optionId(filtered[active].id) : undefined;

  function onKeyDown(event: KeyboardEvent) {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setActive((index) => Math.min(index + 1, filtered.length - 1));
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      setActive((index) => Math.max(index - 1, 0));
    }
    if (event.key === "Enter") {
      event.preventDefault();
      filtered[active]?.run();
    }
  }

  let cursor = 0;
  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="command-overlay" />
        <Dialog.Content
          className="flick-glass command-palette"
          aria-describedby="command-description"
          onKeyDown={onKeyDown}
        >
          <div className="command-title-row">
            <div>
              <Dialog.Title>Command palette</Dialog.Title>
              <Dialog.Description id="command-description">
                Search routes and actions. Use arrow keys and Enter.
              </Dialog.Description>
            </div>
            <Button variant="ghost" size="sm" onClick={() => onOpenChange(false)}>
              Close
            </Button>
          </div>
          <label className="command-search">
            <span>Search Flick commands</span>
            <input
              ref={inputRef}
              role="combobox"
              aria-expanded={open}
              aria-controls="command-results"
              aria-activedescendant={activeOptionId}
              aria-autocomplete="list"
              value={query}
              onChange={(event) => {
                setQuery(event.currentTarget.value);
                setActive(0);
              }}
              placeholder="Search Flick commands"
            />
            <Kbd>⌘K</Kbd>
          </label>
          <div className="command-results" id="command-results" role="listbox" aria-label="Commands">
            {Object.entries(grouped).map(([group, items]) => (
              <div className="command-group" key={group}>
                <span>{group}</span>
                {items.map((item) => {
                  const index = cursor++;
                  return (
                    <button
                      key={item.id}
                      id={optionId(item.id)}
                      type="button"
                      className="command-row"
                      role="option"
                      aria-selected={index === active}
                      data-active={index === active ? "true" : undefined}
                      onMouseEnter={() => setActive(index)}
                      onClick={item.run}
                    >
                      <strong>{item.label}</strong>
                      <small>{item.description}</small>
                      {item.shortcut ? <Kbd>{item.shortcut}</Kbd> : null}
                    </button>
                  );
                })}
              </div>
            ))}
            {!filtered.length ? <p className="command-empty">No commands match that search.</p> : null}
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
