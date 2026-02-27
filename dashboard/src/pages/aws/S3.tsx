import { api, S3Bucket } from "../../api/client";
import { useApi } from "../../hooks/useApi";
import { PageHeader } from "../../components/shared/PageHeader";
import { StatCard } from "../../components/shared/StatCard";
import { ResourceTable, Column } from "../../components/shared/ResourceTable";
import { EmptyState } from "../../components/shared/EmptyState";
import { ErrorBanner } from "../../components/shared/ErrorBanner";
import { Spinner } from "../../components/shared/Spinner";
import { Badge } from "../../components/shared/Badge";
import { RefreshButton } from "../../components/shared/RefreshButton";

export function S3Page() {
  const [state, refetch] = useApi(api.aws.s3);

  return (
    <div>
      <PageHeader
        title="Simple Storage Service"
        subtitle="AWS S3 — buckets and objects"
        badge={<Badge label="REST" variant="yellow" />}
        actions={<RefreshButton onClick={refetch} />}
      />

      {state.status === "loading" && <Spinner />}
      {state.status === "error" && (
        <ErrorBanner message={state.error} onRetry={refetch} />
      )}

      {state.status === "success" && (
        <div className="px-8 py-6 space-y-8">
          <div className="grid grid-cols-1 gap-4 max-w-xs">
            <StatCard
              label="Buckets"
              value={state.data.buckets.length}
              accent="text-orange-400"
            />
          </div>

          <section>
            <h2 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-3">
              Buckets
            </h2>
            {state.data.buckets.length === 0 ? (
              <EmptyState title="No buckets" message="Create a bucket with the S3 SDK." />
            ) : (
              <div className="bg-gray-900 border border-gray-800 rounded-xl overflow-hidden">
                <ResourceTable<S3Bucket>
                  keyFn={(r) => r.name}
                  rows={state.data.buckets}
                  columns={bucketColumns}
                />
              </div>
            )}
          </section>
        </div>
      )}
    </div>
  );
}

const bucketColumns: Column<S3Bucket>[] = [
  {
    header: "Name",
    render: (r) => <span className="font-mono font-medium">{r.name}</span>,
  },
  {
    header: "Created",
    render: (r) => (
      <span className="text-gray-500 text-xs">{new Date(r.created_at).toLocaleString()}</span>
    ),
  },
];
