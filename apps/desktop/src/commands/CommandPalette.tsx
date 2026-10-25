// M007-D: the desktop command palette.
//
// The palette is fed by the daemon's catalog and by nothing else. It holds
// no command list, no command names and no command-kind table of its own:
// every row it shows is a row the daemon said exists.
//
// ADR-0052 (accepted 2026-10-24) splits the work:
//
//   * a **frontend effect** changes only what the frontend is showing. The
//     renderer dispatches it locally, from the daemon's declared
//     frontend-action set.
//   * a **daemon effect** needs the daemon: it mutates daemon-owned state,
//     reads daemon-owned state as its result, or drives a daemon-mediated
//     process. The renderer sends it to the daemon, which authorizes it
//     under `command.execute` and audits it as a canonical run.
//
// Free text that resolves to no catalog entry stays prompt text. There is
// no fall-through: an unrecognised line is never treated as a command.

import { useCallback, useEffect, useMemo, useState } from 'react';
import type {
  CommandCatalogView,
  CommandDescriptorView,
  CommandExecuteView,
} from '../bridge-types';
import { bridge } from '../bridge';

export interface CommandPaletteProps {
  /** Whether the palette is open. */
  open: boolean;
  /** The route generation the catalog read and any execution are fenced on. */
  routeGeneration: number | undefined;
  /** Close the palette. */
  onClose: () => void;
  /** A frontend effect the palette dispatched locally. */
  onFrontendEffect?: (command: CommandDescriptorView) => void;
  /** A daemon-effect execution the daemon answered. */
  onDaemonResult?: (result: CommandExecuteView) => void;
}

/** One palette row: a catalog command and the text the user typed. */
interface PaletteRow {
  command: CommandDescriptorView;
  /** The argument the user typed after the command's spelling. */
  argument: string;
}

/**
 * Resolve a typed line to a catalog entry.
 *
 * The line's first token is matched against the catalog's names and
 * aliases. A line that matches nothing is not a command: it is prompt
 * text, and the caller sends it as such.
 */
export function resolveTypedLine(
  catalog: CommandCatalogView | null,
  typed: string,
): PaletteRow | null {
  if (!catalog) return null;
  const trimmed = typed.trim();
  if (!trimmed.startsWith('/')) return null;
  const separator = trimmed.search(/\s/);
  const spelling = separator === -1 ? trimmed : trimmed.slice(0, separator);
  const argument = separator === -1 ? '' : trimmed.slice(separator + 1).trim();
  const command = catalog.commands.find(
    (candidate) =>
      candidate.name === spelling || candidate.aliases.includes(spelling),
  );
  if (!command) return null;
  return { command, argument };
}

/**
 * The form a typed argument selects.
 *
 * A command's argument can select a different effect, so the form is
 * resolved from the argument when the command declares more than one. The
 * empty form is the command's default form.
 */
export function resolveForm(command: CommandDescriptorView, argument: string): string {
  if (command.forms.length <= 1) return command.forms[0]?.form ?? '';
  const token = argument.split(/\s/)[0] ?? '';
  const matched = command.forms.find((form) => form.form !== '' && form.form === token);
  if (matched) return matched.form;
  return command.forms.find((form) => form.form === '')?.form ?? command.forms[0].form;
}

/** The catalog rows that match the typed text, in catalog order. */
export function filterCommands(
  catalog: CommandCatalogView | null,
  typed: string,
): CommandDescriptorView[] {
  if (!catalog) return [];
  const needle = typed.trim().toLowerCase();
  if (!needle) return catalog.commands;
  return catalog.commands.filter((command) => {
    // The typed text may or may not carry the leading slash, and the
    // catalog's spellings always do, so both sides are compared without it.
    const spellings = [command.name, ...command.aliases].map((spelling) =>
      spelling.replace(/^\//, '').toLowerCase(),
    );
    if (spellings.some((spelling) => spelling.startsWith(needle))) return true;
    return command.description.toLowerCase().includes(needle);
  });
}

export function CommandPalette({
  open,
  routeGeneration,
  onClose,
  onFrontendEffect,
  onDaemonResult,
}: CommandPaletteProps) {
  const [catalog, setCatalog] = useState<CommandCatalogView | null>(null);
  const [typed, setTyped] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) return;
    setError(null);
    if (routeGeneration === undefined) return;
    let mounted = true;
    void bridge
      .commandCatalog(routeGeneration)
      .then((view) => {
        if (mounted) setCatalog(view);
      })
      .catch((cause: unknown) => {
        if (mounted) setError(cause instanceof Error ? cause.message : String(cause));
      });
    return () => {
      mounted = false;
    };
  }, [open, routeGeneration]);

  const rows = useMemo(() => filterCommands(catalog, typed), [catalog, typed]);

  const submit = useCallback(async () => {
    if (!catalog || routeGeneration === undefined) return;
    const resolved = resolveTypedLine(catalog, typed);
    // Free text that names no catalog entry stays prompt text. It is
    // never dispatched as a command.
    if (!resolved) {
      onClose();
      return;
    }
    const form = resolveForm(resolved.command, resolved.argument);
    const shape = resolved.command.forms.find((candidate) => candidate.form === form);
    if (!shape) return;
    if (shape.kind === 'frontend_effect') {
      // A frontend effect never reaches the daemon: the frontend owns it.
      onFrontendEffect?.(resolved.command);
      onClose();
      return;
    }
    if (!shape.daemonServed) {
      // The daemon's catalog says it does not serve this form yet. The
      // renderer says so rather than dispatching into a refusal.
      setError(`${resolved.command.name} is not served by the daemon yet`);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const result = await bridge.commandExecute(
        resolved.command.commandId,
        form,
        resolved.argument,
        catalog.catalogVersion,
        routeGeneration,
      );
      onDaemonResult?.(result);
    } catch (cause: unknown) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  }, [catalog, onClose, onFrontendEffect, onDaemonResult, routeGeneration, typed]);

  if (!open) return null;

  return (
    <div className="cg-palette" role="dialog" aria-label="Command palette">
      <input
        className="cg-palette-input"
        value={typed}
        autoFocus
        placeholder="Type a command"
        onChange={(event) => setTyped(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.preventDefault();
            onClose();
          }
          if (event.key === 'Enter') {
            event.preventDefault();
            void submit();
          }
        }}
      />
      {error ? <p className="cg-palette-error">{error}</p> : null}
      <ul className="cg-palette-list">
        {rows.map((command) => (
          <li key={command.commandId} className="cg-palette-row">
            <span className="cg-palette-name">{command.name}</span>
            <span className="cg-palette-description">{command.description}</span>
          </li>
        ))}
      </ul>
      {busy ? <p className="cg-palette-busy">Running…</p> : null}
    </div>
  );
}
