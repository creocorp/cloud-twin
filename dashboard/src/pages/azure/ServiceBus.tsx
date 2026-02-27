import { api, AsbQueue, AsbTopic } from "../../api/client";
import { useApi } from "../../hooks/useApi";
import { PageHeader } from "../../components/shared/PageHeader";
import { StatCard } from "../../components/shared/StatCard";
import { ResourceTable, Column } from "../../components/shared/ResourceTable";
import { EmptyState } from "../../components/shared/EmptyState";
import { ErrorBanner } from "../../components/shared/ErrorBanner";
import { Spinner } from "../../components/shared/Spinner";
import { Badge } from "../../components/shared/Badge";
import { RefreshButton } from "../../components/shared/RefreshButton";

export function ServiceBusPage() {
  const [state, refetch] = useApi(api.azure.serviceBus);

  return (
    <div>
      <PageHeader
        title="Service Bus"
        subtitle="Azure Service Bus — queues and topics"
        badge={<Badge label="Azure" variant="blue" />}
        actions={<RefreshButton onClick={refetch} />}
      />

      {state.status === "loading" && <Spinner />}
      {state.status === "error" && (
        <ErrorBanner message={state.error} onRetry={refetch} />
      )}

      {state.status === "success" && (
        <div className="px-8 py-6 space-y-8">
          <div className="grid grid-cols-2 gap-4 max-w-md">
            <StatCard label="Queues" value={state.data.queues.length} accent="text-blue-400" />
            <StatCard label="Topics" value={state.data.topics.length} accent="text-blue-400" />
          </div>

          <section>
            <h2 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-3">Queues</h2>
            {state.data.queues.length === 0 ? (
              <EmptyState title="No queues" message="Create a queue with the Azure Service Bus SDK." />
            ) : (
              <div className="bg-gray-900 border border-gray-800 rounded-xl overflow-hidden">
                <ResourceTable<AsbQueue>
                  keyFn={(r) => r.name}
                  rows={state.data.queues}
                  columns={queueColumns}
                />
              </div>
            )}
          </section>

          <section>
            <h2 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-3">Topics</h2>
            {state.data.topics.length === 0 ? (
              <EmptyState title="No topics" message="Create a topic with the Azure Service Bus SDK." />
            ) : (
              <div className="bg-gray-900 border border-gray-800 rounded-xl overflow-hidden">
                <ResourceTable<AsbTopic>
                  keyFn={(r) => r.name}
                  rows={state.data.topics}
                  columns={topicColumns}
                />
              </div>
            )}
          </section>
        </div>
      )}
    </div>
  );
}

const queueColumns: Column<AsbQueue>[] = [
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
    header: "Created",
    render: (r) => <span className="text-gray-500 text-xs">{new Date(r.created_at).toLocaleString()}</span>,
  },
];

const topicColumns: Column<AsbTopic>[] = [
  { header: "Name", render: (r) => <span className="font-medium">{r.name}</span> },
  {
    header: "Subscriptions",
    render: (r) => (
      <span className={r.subscription_count > 0 ? "text-blue-400 font-medium" : "text-gray-500"}>
        {r.subscription_count}
      </span>
    ),
  },
  {
    header: "Created",
    render: (r) => <span className="text-gray-500 text-xs">{new Date(r.created_at).toLocaleString()}</span>,
  },
];
