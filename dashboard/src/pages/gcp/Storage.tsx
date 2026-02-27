import { api, GcsBucket } from "../../api/client";
import { useApi } from "../../hooks/useApi";
import { PageHeader } from "../../components/shared/PageHeader";
import { StatCard } from "../../components/shared/StatCard";
import { ResourceTable, Column } from "../../components/shared/ResourceTable";
import { EmptyState } from "../../components/shared/EmptyState";
import { ErrorBanner } from "../../components/shared/ErrorBanner";
import { Spinner } from "../../components/shared/Spinner";
import { Badge } from "../../components/shared/Badge";
import { RefreshButton } from "../../components/shared/RefreshButton";

export function GCSPage() {
  const [state, refetch] = useApi(api.gcp.storage);

  return (
    <div>
      <PageHeader
        title="Cloud Storage"
        subtitle="GCP Cloud Storage — buckets and objects"
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
            <StatCard label="Buckets" value={state.data.buckets.length} accent="text-sky-400" />
            <StatCard
              label="Total objects"
              value={state.data.buckets.reduce((s, b) => s + b.object_count, 0)}
              accent="text-sky-400"
            />
          </div>

          <section>
            <h2 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-3">Buckets</h2>
            {state.data.buckets.length === 0 ? (
              <EmptyState title="No buckets" message="Create a bucket with the GCP Storage SDK." />
            ) : (
              <div className="bg-gray-900 border border-gray-800 rounded-xl overflow-hidden">
                <ResourceTable<GcsBucket>
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

const bucketColumns: Column<GcsBucket>[] = [
  { header: "Name", render: (r) => <span className="font-medium">{r.name}</span> },
  { header: "Location", render: (r) => <Badge label={r.location} variant="gray" /> },
  {
    header: "Objects",
    render: (r) => (
      <span className={r.object_count > 0 ? "text-sky-400 font-medium" : "text-gray-500"}>
        {r.object_count}
      </span>
    ),
  },
  {
    header: "Created",
    render: (r) => <span className="text-gray-500 text-xs">{new Date(r.created_at).toLocaleString()}</span>,
  },
];
