export type LiveWorkspaceStatus = {
  schemaVersion: string;
  state: "connecting" | "observing" | "unavailable" | "expired" | string;
  provider: string;
  mode: "observe" | "interactive" | string;
  workspace: string;
  capabilities: string[];
  message?: string | null;
};

export type LiveWorkspaceEntry = {
  path: string;
  name: string;
  kind: "directory" | "file" | "symlink" | string;
  size: number;
  modifiedAt?: string | null;
};

export type LiveWorkspaceList = {
  schemaVersion: string;
  path: string;
  entries: LiveWorkspaceEntry[];
  truncated: boolean;
};

export type LiveWorkspaceRead = {
  schemaVersion: string;
  path: string;
  content?: string | null;
  binary: boolean;
  truncated: boolean;
  byteSize: number;
};

/** Host-scoped port. It contains no credential and cannot select another visual/source. */
export type LiveWorkspaceClient = {
  status(): Promise<LiveWorkspaceStatus>;
  list(path: string): Promise<LiveWorkspaceList>;
  read(path: string): Promise<LiveWorkspaceRead>;
};
