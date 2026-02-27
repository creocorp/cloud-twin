import { api, PubsubTopic, PubsubSubscription } from "../../api/client";
import { useApi } from "../../hooks/useApi";
import { PageHeader } from "../../components/shared/PageHeader";
import { StatCard } from "../../components/shared/StatCard";
import { ResourceTable, Column } from "../../components/shared/ResourceTable";
import { EmptyState } from "../../components/shared/EmptyState";
import { ErrorBanner } from "../../components/shared/ErrorBanner";
import { Spinner } from "../../components/shared/Spinner";
import { Badge } from "../../components/shared/Badge";
import { RefreshButton } from "../../components/shared/RefreshButton";

export function PubSubPage() {
  const [state, refetch] = useApi(api.gcp.pubsub);

  return (
    <div>
      <PageHeader
        title="Pub/Sub"
        subtitle="GCP Pub/Sub — topics and subscriptions"
        badge={<Badge label="GCP" variant="blue" />}
        actions={<RefreshButton onClick={refetch} />}
      />

      {state.status === "loading" && <Spinner />}
      {state.status === "error" && (
        <ErrorBanner message={state.error} onRetry={refetch} />
      )}

      {state.status === "success" && (
        <div className="px-8 py-6 space-y-8">
          <div className="grid grid-cols-2 gap-4 max-w-md">
            <StatCard label="Topics" value={state.data.topics.length} accent="text-sky-400" />
            <StatCard label="Subscriptions" value={state.data.subscriptions.length} accent="text-sky-400" />
          </div>

          <section>
            <h2 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-3">Topics</h2>
            {state.data.topics.length === 0 ? (
              <EmptyState title="No topics" message="Create a topic with the GCP Pub/Sub SDK." />
            ) : (
              <div className="bg-gray-900 border border-gray-800 rounded-xl overflow-hidden">
                <ResourceTable<PubsubTopic>
                  keyFn={(r) => r.name}
                  rows={state.data.topics}
                  columns={topicColumns}
                />
              </div>
            )}
          </section>

          <section>
            <h2 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-3">
              Subscriptions
            </h2>
            {state.data.subscriptions.length === 0 ? (
              <EmptyState title="No subscriptions" message="Subscribe to a topic with the GCP Pub/Sub SDK." />
            ) : (
              <div className="bg-gray-900 border border-gray-800 rounded-xl overflow-hidden">
                <ResourceTable<PubsubSubscription>
                  keyFn={(r) => r.name}
                  rows={state.data.subscriptions}
                  columns={subscriptionColumns}
                />
              </div>
            )}
          </section>
        </div>
      )}
    </div>
  );
}

const topicColumns: Column<PubsubTopic>[] = [
  { header: "Name", render: (r) => <span className="font-mono text-sm">{r.name}</span> },
  {
    header: "Subscriptions",
    render: (r) => (
      <span className={r.subscription_count > 0 ? "text-sky-400 font-medium" : "text-gray-500"}>
        {r.subscription_count}
      </span>
    ),
  },
  {
    header: "Created",
    render: (r) => <span className="text-gray-500 text-xs">{new Date(r.created_at).toLocaleString()}</span>,
  },
];

const subscriptionColumns: Column<PubsubSubscription>[] = [
  { header: "Name", render: (r) => <span className="font-mono text-sm">{r.name}</span> },
  {
    header: "Topic",
    render: (r) => <span className="font-mono text-xs text-gray-500">{r.topic.split("/").pop()}</span>,
  },
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
