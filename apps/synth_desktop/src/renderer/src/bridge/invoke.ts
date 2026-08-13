/**
 * Typed invoke skeleton (Wave 2 interim).
 * Prefer `invokeCommand(COMMANDS.X, args)` over raw string command names.
 * Specta seed bindings live in `generated/protocol.ts`; migrate call sites
 * command-by-command onto `spectaCommands` without dropping this helper.
 */

import { invoke as tauriInvoke } from "@tauri-apps/api/core";
export { commands as generatedCommands } from "../generated/protocol";
import { COMMANDS, type CommandName } from "./protocolConstants";

export type InvokeArgs = Record<string, unknown> | undefined;

/**
 * Invoke a Tauri command by const name from {@link COMMANDS}.
 * New bridge call sites should use this instead of string literals.
 */
export function invokeCommand<T>(command: CommandName, args?: InvokeArgs): Promise<T> {
	return tauriInvoke<T>(command, args);
}

export { COMMANDS };
