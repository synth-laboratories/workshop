export { COMMANDS, EVENT_CHANNELS, EVENT_ORIGINS, type CommandName, type EventChannelName, type EventOrigin } from "./protocolConstants";
export { invokeCommand } from "./invoke";
export type * from "./types";
/** Specta-generated subset — grow as commands migrate into `collect_commands!`. */
export { commands as spectaCommands } from "../generated/protocol";
export type { InstanceDiagnostics as SpectaInstanceDiagnostics } from "../generated/protocol";
