import { api, AzureContainer } from "../../api/client";
import { useApi } from "../../hooks/useApi";
import { PageHeader } from "../../components/shared/PageHeader";
import { StatCard } from "../../components/shared/StatCard";
import { ResourceTable, Column } from "../../components/shared/ResourceTable";
import { EmptyState } from "../../components/shared/EmptyState";
import { ErrorBanner } from "../../components/shared/ErrorBanner";
import { Spinner } from "../../components/shared/Spinner";
import { Badge } from "../../components/shared/Badge";
import { RefreshButton } from "../../components/shared/RefreshButton";

export function AzureBlobPage() {
  const [state, refetch] = useApi(api.azure.blob);

  return (
    <div>
      <PageHeader
        title="Blob Storage"
        subtitle="Azure Blob Storage — containers and blobs"
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
            <StatCard label="Containers" value={state.data.containers.length} accent="text-blue-400" />
            <StatCard
              label="Total blobs"
              value={state.data.containers.reduce((s, c) => s + c.blob_count, 0)}
              accent="text-blue-400"
            />
          </div>

          <section>
            <h2 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-3">
              Containers
            </h2>
            {state.data.containers.length === 0 ? (
              <EmptyState title="No containers" message="Create a container with the Azure Blob SDK." />
            ) : (
              <div className="bg-gray-900 border border-gray-800 rounded-xl overflow-hidden">
                <ResourceTable<AzureContainer>
                  keyFn={(r) => r.name}
                  rows={state.data.containers}
                  columns={containerColumns}
                />
              </div>
            )}
          </section>
        </div>
      )}
    </div>
  );
}

const containerColumns: Column<AzureContainer>[] = [
  { header: "Name", render: (r) => <span className="font-medium">{r.name}</span> },
  {
    header: "Blobs",
    render: (r) => (
      <span className={r.blob_count > 0 ? "text-blue-400 font-medium" : "text-gray-500"}>
        {r.blob_count}
      </span>
    ),
  },
  {
    header: "Created",
    render: (r) => <span className="text-gray-500 text-xs">{new Date(r.created_at).toLocaleString()}</span>,
  },
];
