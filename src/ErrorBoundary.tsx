import React from "react";

type State = { failed: boolean };

export default class ErrorBoundary extends React.Component<React.PropsWithChildren, State> {
  state: State = { failed: false };

  static getDerivedStateFromError(): State {
    return { failed: true };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    console.error("TidyBeam UI error", error, info.componentStack);
  }

  render() {
    if (this.state.failed) {
      return (
        <main className="crash" role="alert">
          <div className="crash-card">
            <h1>Something went wrong</h1>
            <p>Your files weren't changed. Reload TidyBeam, then check Transfers for anything that was in progress.</p>
            <button type="button" className="btn btn-primary" onClick={() => window.location.reload()}>Reload</button>
          </div>
        </main>
      );
    }
    return this.props.children;
  }
}
