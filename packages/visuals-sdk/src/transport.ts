import type { PresentationState, VisualRecording, VisualSnapshot } from "@synth/visuals-protocol";

export interface VisualStateTransport {
  loadPresentation(visualId: string): Promise<PresentationState | undefined>;
  savePresentation(visualId: string, state: PresentationState, expectedStateVersion: number): Promise<PresentationState>;
  saveSnapshot(snapshot: VisualSnapshot): Promise<VisualSnapshot>;
  listSnapshots(visualId: string): Promise<VisualSnapshot[]>;
  saveRecording(recording: VisualRecording): Promise<VisualRecording>;
  listRecordings(visualId: string): Promise<VisualRecording[]>;
}

export type FetchLike = (input: string, init?: RequestInit) => Promise<Response>;

export class HttpVisualStateTransport implements VisualStateTransport {
  readonly baseUrl: string;
  readonly fetcher: FetchLike;
  constructor(baseUrl: string, fetcher: FetchLike = fetch) {
    this.baseUrl = baseUrl;
    this.fetcher = fetcher;
  }

  async #request<T>(path: string, init?: RequestInit): Promise<T> {
    const response = await this.fetcher(`${this.baseUrl.replace(/\/$/, "")}${path}`, {
      ...init,
      headers: { "content-type": "application/json", ...init?.headers },
    });
    const body = await response.json() as Record<string, unknown>;
    if (!response.ok) throw new Error(String(body.error ?? body.detail ?? `visual state request failed (${response.status})`));
    return body as T;
  }

  async loadPresentation(visualId: string): Promise<PresentationState | undefined> {
    const body = await this.#request<{ presentation?: PresentationState }>(`/v1/visuals/${encodeURIComponent(visualId)}/presentation`);
    return body.presentation;
  }
  async savePresentation(visualId: string, state: PresentationState, expectedStateVersion: number): Promise<PresentationState> {
    const body = await this.#request<{ presentation: PresentationState }>(`/v1/visuals/${encodeURIComponent(visualId)}/presentation`, {
      method: "POST", body: JSON.stringify({ ...state, expectedStateVersion }),
    });
    return body.presentation;
  }
  async saveSnapshot(snapshot: VisualSnapshot): Promise<VisualSnapshot> {
    const body = await this.#request<{ snapshot: VisualSnapshot }>(`/v1/visuals/${encodeURIComponent(snapshot.visualId)}/snapshots`, { method: "POST", body: JSON.stringify(snapshot) });
    return body.snapshot;
  }
  async listSnapshots(visualId: string): Promise<VisualSnapshot[]> {
    const body = await this.#request<{ snapshots: VisualSnapshot[] }>(`/v1/visuals/${encodeURIComponent(visualId)}/snapshots`);
    return body.snapshots;
  }
  async saveRecording(recording: VisualRecording): Promise<VisualRecording> {
    const body = await this.#request<{ recording: VisualRecording }>(`/v1/visuals/${encodeURIComponent(recording.visualId)}/recordings`, { method: "POST", body: JSON.stringify(recording) });
    return body.recording;
  }
  async listRecordings(visualId: string): Promise<VisualRecording[]> {
    const body = await this.#request<{ recordings: VisualRecording[] }>(`/v1/visuals/${encodeURIComponent(visualId)}/recordings`);
    return body.recordings;
  }
}
