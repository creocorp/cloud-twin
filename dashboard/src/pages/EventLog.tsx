import { api, TelemetryEvent } from "../api/client";
import { usePolling } from "../hooks/useApi";
import { PageHeader } from "../components/shared/PageHeader";
import { EmptyState } from "../components/shared/EmptyState";
import { ErrorBanner } from "../components/shared/ErrorBanner";
import { Spinner } from "../components/shared/Spinner";
import { Badge } from "../components/shared/Badge";
import { RefreshButton } from "../components/shared/RefreshButton";

const providerBadge: Record<string, "yellow" | "blue" | "blue" | "gray"> = {
  aws: "yellow",
  azure: "blue",
  gcp: "blue",
};

function EventRow({ event }: { event: TelemetryEvent }) {
  return (
    <div className="flex items-start gap-4 px-5 py-3 hover:bg-gray-800/30 transition-colors border-b border-gray-800/60 last:border-0">
      <span className="text-xs text-gray-600 font-mono w-24 shrink-0 mt-0.5">
        {new Date(event.created_at).toLocaleTimeString()}
      </span>
      <div className="flex items-center gap-2 shrink-0">
        <Badge label={event.provider} variant={providerBadge[event.provider] ?? "gray"} />
        <Badge label={event.service} variant="gray" />
      </div>
      <span className="text-sm text-gray-300 font-mono">{event.action}</span>
      <span className="text-xs text-gray-600 font-mono truncate flex-1">
        {JSON.stringify(event.payload)}
      </span>
    </div>
  );
}

export function EventLogPage() {
  const [state, refetch] = usePolling(() => api.events(100), 3000);

  const events =
    state.status === "success" ? state.data.events : [];

  return (
    <div>
      <PageHeader
        title="Event Log"
        subtitle="Live telemetry from all services — auto-refreshes every 3 seconds"
        actions={
          <div className="flex items-center gap-3">
            {state.status === "success" && (
              <span className="text-xs text-gray-600">{events.length} events</span>
            )}
            <RefreshButton onClick={refetch} />
          </div>
        }
      />

      {state.status === "loading" && <Spinner />}
      {state.status === "error" && (
        <ErrorBanner message={state.error} onRetry={refetch} />
      )}

      {state.status === "success" && (
        <div className="px-8 py-6">
          {events.length === 0 ? (
            <EmptyState
              title="No events yet"
              message="Events will appear here as you interact with the services."
            />
          ) : (
            <div className="bg-gray-900 border border-gray-800 rounded-xl overflow-hidden">
              <div className="flex items-center gap-4 px-5 py-2 border-b border-gray-800 bg-gray-900/80">
                <span className="text-xs text-gray-600 w-24 shrink-0">Time</span>
                <span className="text-xs text-gray-600 w-32 shrink-0">Provider / Service</span>
                <span className="text-xs text-gray-600">Action</span>
                <span className="text-xs text-gray-600">Payload</span>
              </div>
              {[...events].reverse().map((ev) => (
                <EventRow key={ev.id} event={ev} />
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
