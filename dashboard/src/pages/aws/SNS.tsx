import { api, SnsTopic, SnsSubscription } from "../../api/client";
import { useApi } from "../../hooks/useApi";
import { PageHeader } from "../../components/shared/PageHeader";
import { StatCard } from "../../components/shared/StatCard";
import { ResourceTable, Column } from "../../components/shared/ResourceTable";
import { EmptyState } from "../../components/shared/EmptyState";
import { ErrorBanner } from "../../components/shared/ErrorBanner";
import { Spinner } from "../../components/shared/Spinner";
import { Badge } from "../../components/shared/Badge";
import { RefreshButton } from "../../components/shared/RefreshButton";

export function SNSPage() {
  const [state, refetch] = useApi(api.aws.sns);

  return (
    <div>
      <PageHeader
        title="Simple Notification Service"
        subtitle="AWS SNS — topics and subscriptions"
        badge={<Badge label="Query" variant="yellow" />}
        actions={<RefreshButton onClick={refetch} />}
      />

      {state.status === "loading" && <Spinner />}
      {state.status === "error" && (
        <ErrorBanner message={state.error} onRetry={refetch} />
      )}

      {state.status === "success" && (
        <div className="px-8 py-6 space-y-8">
          <div className="grid grid-cols-2 gap-4 max-w-md">
            <StatCard label="Topics" value={state.data.topics.length} accent="text-orange-400" />
            <StatCard label="Subscriptions" value={state.data.subscriptions.length} accent="text-orange-400" />
          </div>

          <section>
            <h2 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-3">Topics</h2>
            {state.data.topics.length === 0 ? (
              <EmptyState title="No topics" message="Create a topic with the SNS SDK." />
            ) : (
              <div className="bg-gray-900 border border-gray-800 rounded-xl overflow-hidden">
                <ResourceTable<SnsTopic>
                  keyFn={(r) => r.arn}
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
              <EmptyState title="No subscriptions" message="Subscribe an endpoint to a topic." />
            ) : (
              <div className="bg-gray-900 border border-gray-800 rounded-xl overflow-hidden">
                <ResourceTable<SnsSubscription>
                  keyFn={(r) => r.arn}
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

const topicColumns: Column<SnsTopic>[] = [
  { header: "Name", render: (r) => <span className="font-medium">{r.name}</span> },
  { header: "ARN", render: (r) => <span className="font-mono text-xs text-gray-500">{r.arn}</span> },
  {
    header: "Created",
    render: (r) => <span className="text-gray-500 text-xs">{new Date(r.created_at).toLocaleString()}</span>,
  },
];

const subscriptionColumns: Column<SnsSubscription>[] = [
  { header: "Protocol", render: (r) => <Badge label={r.protocol} variant="blue" /> },
  { header: "Endpoint", render: (r) => <span className="font-mono text-sm">{r.endpoint}</span> },
  {
    header: "Topic",
    render: (r) => <span className="font-mono text-xs text-gray-500">{r.topic_arn.split(":").pop()}</span>,
  },
];
