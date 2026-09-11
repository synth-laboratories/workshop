import { fromGenerated } from "../bridge/invoke";
import { listen } from "@tauri-apps/api/event";
import { commands, type ScopeView } from "../generated/protocol";
import { EVENT_CHANNELS } from "../bridge/protocolConstants";
import { connectScopedHistory, type ScopedHistoryState } from "../stores/scopedCloudHistory";

/** Read-only bootstrap. The host remains qualification-gated. */
export function observeScopedCloudHistory(publish: (state: ScopedHistoryState) => void): () => void {
  return connectScopedHistory({
    observe: listener => listen<ScopeView>(EVENT_CHANNELS.CLOUD_SCOPE, event => listener(event.payload)),
    view: () => fromGenerated(commands.cloudScopeView()),
    history: () => fromGenerated(commands.cloudScopedHistory())
  }, publish);
}
