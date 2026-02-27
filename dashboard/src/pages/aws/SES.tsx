import { api, SesIdentity, SesMessage } from "../../api/client";
import { useApi } from "../../hooks/useApi";
import { PageHeader } from "../../components/shared/PageHeader";
import { StatCard } from "../../components/shared/StatCard";
import { ResourceTable, Column } from "../../components/shared/ResourceTable";
import { EmptyState } from "../../components/shared/EmptyState";
import { ErrorBanner } from "../../components/shared/ErrorBanner";
import { Spinner } from "../../components/shared/Spinner";
import { Badge } from "../../components/shared/Badge";
import { RefreshButton } from "../../components/shared/RefreshButton";

export function SESPage() {
  const [state, refetch] = useApi(api.aws.ses);

  return (
    <div>
      <PageHeader
        title="Simple Email Service"
        subtitle="AWS SES — identities and sent messages"
        badge={<Badge label="v1 + v2" variant="yellow" />}
        actions={<RefreshButton onClick={refetch} />}
      />

      {state.status === "loading" && <Spinner />}
      {state.status === "error" && (
        <ErrorBanner message={state.error} onRetry={refetch} />
      )}

      {state.status === "success" && (
        <div className="px-8 py-6 space-y-8">
          {/* Stats */}
          <div className="grid grid-cols-2 gap-4 max-w-md">
            <StatCard
              label="Identities"
              value={state.data.identities.length}
              accent="text-orange-400"
            />
            <StatCard
              label="Messages sent"
              value={state.data.messages.length}
              accent="text-orange-400"
            />
          </div>

          {/* Identities */}
          <section>
            <h2 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-3">
              Verified Identities
            </h2>
            {state.data.identities.length === 0 ? (
              <EmptyState title="No identities" message="Verify an email or domain to get started." />
            ) : (
              <div className="bg-gray-900 border border-gray-800 rounded-xl overflow-hidden">
                <ResourceTable<SesIdentity>
                  keyFn={(r) => r.identity}
                  rows={state.data.identities}
                  columns={identityColumns}
                />
              </div>
            )}
          </section>

          {/* Recent messages */}
          <section>
            <h2 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-3">
              Recent Messages
            </h2>
            {state.data.messages.length === 0 ? (
              <EmptyState title="No messages" message="No emails sent yet." />
            ) : (
              <div className="bg-gray-900 border border-gray-800 rounded-xl overflow-hidden">
                <ResourceTable<SesMessage>
                  keyFn={(r) => String(r.id)}
                  rows={state.data.messages}
                  columns={messageColumns}
                />
              </div>
            )}
          </section>
        </div>
      )}
    </div>
  );
}

const identityColumns: Column<SesIdentity>[] = [
  {
    header: "Identity",
    render: (r) => <span className="font-mono text-sm">{r.identity}</span>,
  },
  {
    header: "Type",
    render: (r) => <Badge label={r.type} variant={r.type === "email" ? "blue" : "gray"} />,
  },
  {
    header: "Status",
    render: (r) => (
      <Badge label={r.verified ? "verified" : "pending"} variant={r.verified ? "green" : "yellow"} />
    ),
  },
  {
    header: "Created",
    render: (r) => <span className="text-gray-500 text-xs">{new Date(r.created_at).toLocaleString()}</span>,
  },
];

const messageColumns: Column<SesMessage>[] = [
  { header: "#", render: (r) => <span className="text-gray-500">{r.id}</span> },
  { header: "From", render: (r) => <span className="font-mono text-sm">{r.source}</span> },
  { header: "To", render: (r) => <span className="font-mono text-sm">{r.destination}</span> },
  { header: "Subject", render: (r) => r.subject },
  {
    header: "Sent",
    render: (r) => <span className="text-gray-500 text-xs">{new Date(r.created_at).toLocaleString()}</span>,
  },
];
