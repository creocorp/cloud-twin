import { api, SqsQueue } from "../../api/client";
import { useApi } from "../../hooks/useApi";
import { PageHeader } from "../../components/shared/PageHeader";
import { StatCard } from "../../components/shared/StatCard";
import { ResourceTable, Column } from "../../components/shared/ResourceTable";
import { EmptyState } from "../../components/shared/EmptyState";
import { ErrorBanner } from "../../components/shared/ErrorBanner";
import { Spinner } from "../../components/shared/Spinner";
import { Badge } from "../../components/shared/Badge";
import { RefreshButton } from "../../components/shared/RefreshButton";

export function SQSPage() {
  const [state, refetch] = useApi(api.aws.sqs);

  return (
    <div>
      <PageHeader
        title="Simple Queue Service"
        subtitle="AWS SQS — queues and message depth"
        badge={<Badge label="JSON" variant="yellow" />}
        actions={<RefreshButton onClick={refetch} />}
      />

      {state.status === "loading" && <Spinner />}
      {state.status === "error" && (
        <ErrorBanner message={state.error} onRetry={refetch} />
      )}

      {state.status === "success" && (
        <div className="px-8 py-6 space-y-8">
          <div className="grid grid-cols-2 gap-4 max-w-md">
            <StatCard label="Queues" value={state.data.queues.length} accent="text-orange-400" />
            <StatCard
              label="Total messages"
              value={state.data.queues.reduce((s, q) => s + q.message_count, 0)}
              accent="text-orange-400"
            />
          </div>

          <section>
            <h2 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-3">Queues</h2>
            {state.data.queues.length === 0 ? (
              <EmptyState title="No queues" message="Create a queue with the SQS SDK." />
            ) : (
              <div className="bg-gray-900 border border-gray-800 rounded-xl overflow-hidden">
                <ResourceTable<SqsQueue>
                  keyFn={(r) => r.url}
                  rows={state.data.queues}
                  columns={queueColumns}
                />
              </div>
            )}
          </section>
        </div>
      )}
    </div>
  );
}

const queueColumns: Column<SqsQueue>[] = [
  { header: "Name", render: (r) => <span className="font-medium">{r.name}</span> },
  {
    header: "Messages",
    render: (r) => (
      <span className={r.message_count > 0 ? "text-yellow-400 font-medium" : "text-gray-500"}>
        {r.message_count}
      </span>
    ),
  },
  {
    header: "URL",
    render: (r) => <span className="font-mono text-xs text-gray-500 truncate">{r.url}</span>,
  },
  {
    header: "Created",
    render: (r) => <span className="text-gray-500 text-xs">{new Date(r.created_at).toLocaleString()}</span>,
  },
];
