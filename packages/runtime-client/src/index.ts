import type {
  ContainerDeployment,
  EventPage,
  ExecutionTarget,
  RuntimeControlKind,
  RuntimeEvent,
  RuntimeHealth,
  Project,
  Session,
  TraceV5Record,
  UsageLedgerEntry,
  VisualInstanceRecord,
} from "@synth/runtime-protocol";

type RequestOptions = {
  method?: "GET" | "POST" | "DELETE";
  body?: unknown;
};

type EventSubscription = {
  close(): void;
};

type RuntimeBridge = {
  request<T>(path: string, options?: RequestOptions): Promise<T>;
  subscribe(
    sessionId: string,
    afterSequence: number,
    onEvent: (event: RuntimeEvent) => void,
    onStatus?: (status: { state: string; detail?: string }) => void,
  ): Promise<EventSubscription>;
};

declare global {
  interface Window {
    synthRuntime: RuntimeBridge;
  }
}

function bridge(): RuntimeBridge {
  if (!window.synthRuntime) {
    throw new Error("Synth runtime preload bridge is unavailable");
  }
  return window.synthRuntime;
}

export const runtimeClient = {
  health(): Promise<RuntimeHealth> {
    return bridge().request("/v1/health");
  },

  async listSessions(): Promise<Session[]> {
    const response = await bridge().request<{ sessions: Session[] }>("/v1/sessions");
    return response.sessions;
  },

  getSession(sessionId: string): Promise<Session> {
    return bridge().request(`/v1/sessions/${encodeURIComponent(sessionId)}`);
  },

  async listProjects(): Promise<Project[]> {
    const response = await bridge().request<{ projects: Project[] }>("/v1/projects");
    return response.projects;
  },

  createProject(body: {
    path: string;
    name?: string;
    vcs?: string;
    metadata?: Record<string, unknown>;
  }): Promise<Project> {
    return bridge().request("/v1/projects", { method: "POST", body });
  },

  getProject(projectId: string): Promise<Project> {
    return bridge().request(`/v1/projects/${encodeURIComponent(projectId)}`);
  },

  deleteProject(projectId: string): Promise<{ deleted: boolean }> {
    return bridge().request(`/v1/projects/${encodeURIComponent(projectId)}`, {
      method: "DELETE",
    });
  },

  createSession(
    target: ExecutionTarget,
    title?: string,
    projectId?: string | null,
  ): Promise<Session> {
    return bridge().request("/v1/sessions", {
      method: "POST",
      body: { target, title, projectId },
    });
  },

  deleteSession(sessionId: string): Promise<{ deleted: boolean }> {
    return bridge().request(`/v1/sessions/${encodeURIComponent(sessionId)}`, {
      method: "DELETE",
    });
  },

  sendMessage(sessionId: string, body: string): Promise<{ runId: string }> {
    return bridge().request(`/v1/sessions/${encodeURIComponent(sessionId)}/messages`, {
      method: "POST",
      body: { body },
    });
  },

  control(
    sessionId: string,
    kind: RuntimeControlKind,
    payload: Record<string, unknown> = {},
  ): Promise<{ accepted: boolean; receipt?: unknown }> {
    return bridge().request(`/v1/sessions/${encodeURIComponent(sessionId)}/commands`, {
      method: "POST",
      body: { kind, payload },
    });
  },

  events(sessionId: string, afterSequence = 0, limit = 500): Promise<EventPage> {
    const query = new URLSearchParams({
      after_sequence: String(afterSequence),
      limit: String(limit),
    });
    return bridge().request(
      `/v1/sessions/${encodeURIComponent(sessionId)}/events?${query.toString()}`,
    );
  },

  subscribe(
    sessionId: string,
    afterSequence: number,
    onEvent: (event: RuntimeEvent) => void,
    onStatus?: (status: { state: string; detail?: string }) => void,
  ): Promise<EventSubscription> {
    return bridge().subscribe(sessionId, afterSequence, onEvent, onStatus);
  },

  async listContainers(): Promise<ContainerDeployment[]> {
    const response = await bridge().request<{ containers: ContainerDeployment[] }>(
      "/v1/containers",
    );
    return response.containers;
  },

  getContainer(containerId: string): Promise<ContainerDeployment> {
    return bridge().request(`/v1/containers/${encodeURIComponent(containerId)}`);
  },

  probeContainer(containerId: string): Promise<ContainerDeployment> {
    return bridge().request(
      `/v1/containers/${encodeURIComponent(containerId)}/probe`,
      { method: "POST" },
    );
  },

  async listTraces(): Promise<TraceV5Record[]> {
    const response = await bridge().request<{ traces: TraceV5Record[] }>("/v1/traces");
    return response.traces;
  },

  getTrace(traceId: string): Promise<TraceV5Record> {
    return bridge().request(`/v1/traces/${encodeURIComponent(traceId)}`);
  },

  async listVisuals(): Promise<VisualInstanceRecord[]> {
    const response = await bridge().request<{ visuals: VisualInstanceRecord[] }>(
      "/v1/visuals",
    );
    return response.visuals;
  },

  getVisual(visualId: string): Promise<VisualInstanceRecord> {
    return bridge().request(`/v1/visuals/${encodeURIComponent(visualId)}`);
  },

  async listVisualTemplates(): Promise<unknown[]> {
    const response = await bridge().request<{ templates: unknown[] }>(
      "/v1/visuals/templates",
    );
    return response.templates;
  },

  getVisualTemplate(templateId: string): Promise<unknown> {
    return bridge().request(
      `/v1/visuals/templates/${encodeURIComponent(templateId)}`,
    );
  },

  createVisual(body: {
    templateId: string;
    title?: string;
    bindings?: Record<string, unknown>;
    metadata?: Record<string, unknown>;
  }): Promise<VisualInstanceRecord> {
    return bridge().request("/v1/visuals", {
      method: "POST",
      body,
    });
  },

  simulateLive(kind = "eval"): Promise<{ visual: VisualInstanceRecord; eventCount: number }> {
    return bridge().request("/v1/visuals/simulate-live", {
      method: "POST",
      body: { kind },
    });
  },

  async listUsage(limit = 100): Promise<UsageLedgerEntry[]> {
    const query = new URLSearchParams({ limit: String(limit) });
    const response = await bridge().request<{ entries: UsageLedgerEntry[] }>(
      `/v1/usage?${query.toString()}`,
    );
    return response.entries;
  },
};
