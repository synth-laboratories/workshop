import { Component, type ReactNode } from "react";

export class AppErrorBoundary extends Component<{ children: ReactNode }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() { return { failed: true }; }
  render() {
    if (!this.state.failed) return this.props.children;
    return <main className="boot-error" role="alert" style={{ padding: 32 }}>
      <h1>Workshop couldn’t display this screen</h1>
      <p>Reload Workshop to try again.</p>
      <button type="button" className="ws-btn ws-btn-primary" onClick={() => window.location.reload()}>Reload Workshop</button>
    </main>;
  }
}
