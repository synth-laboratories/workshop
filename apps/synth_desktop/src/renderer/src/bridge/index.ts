export { EVENT_CHANNELS, EVENT_ORIGINS, type EventChannelName, type EventOrigin } from "./protocolConstants";
export { fromGenerated, n, wire } from "./invoke";
export type * from "./types";
/** Specta-generated desktop command bindings. */
export { commands as spectaCommands } from "../generated/protocol";
export type { InstanceDiagnostics as SpectaInstanceDiagnostics } from "../generated/protocol";
