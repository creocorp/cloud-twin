import { api } from "../api/client";
import { usePolling } from "../hooks/useApi";
import { Spinner } from "../components/shared/Spinner";
import { ErrorBanner } from "../components/shared/ErrorBanner";
import { Badge } from "../components/shared/Badge";
import { Link } from "react-router-dom";

interface ServiceTileProps {
  name: string;
  provider: string;
  providerColor: string;
  to: string;
  online: boolean;
}

function ServiceTile({ name, provider, providerColor, to, online }: ServiceTileProps) {
  return (
    <Link
      to={to}
      className="bg-gray-900 border border-gray-800 rounded-xl px-5 py-4 hover:border-gray-600 transition-colors group"
    >
      <div className="flex items-center justify-between mb-2">
        <span className={`text-xs font-semibold uppercase tracking-wider ${providerColor}`}>
          {provider}
        </span>
        <span className={`w-2 h-2 rounded-full ${online ? "bg-emerald-400" : "bg-gray-600"}`} />
      </div>
      <p className="text-white font-medium group-hover:text-gray-100">{name}</p>
    </Link>
  );
}

const services: Omit<ServiceTileProps, "online">[] = [
  { name: "SES", provider: "AWS", providerColor: "text-orange-400", to: "/aws/ses" },
  { name: "S3", provider: "AWS", providerColor: "text-orange-400", to: "/aws/s3" },
  { name: "SNS", provider: "AWS", providerColor: "text-orange-400", to: "/aws/sns" },
  { name: "SQS", provider: "AWS", providerColor: "text-orange-400", to: "/aws/sqs" },
  { name: "Blob Storage", provider: "Azure", providerColor: "text-blue-400", to: "/azure/blob" },
  { name: "Service Bus", provider: "Azure", providerColor: "text-blue-400", to: "/azure/servicebus" },
  { name: "Cloud Storage", provider: "GCP", providerColor: "text-sky-400", to: "/gcp/storage" },
  { name: "Pub/Sub", provider: "GCP", providerColor: "text-sky-400", to: "/gcp/pubsub" },
];

export function OverviewPage() {
  const [healthState, refetch] = usePolling(api.health, 10_000);

  const serviceStatus =
    healthState.status === "success" ? healthState.data.services : {};
  const storageMode =
    healthState.status === "success" ? healthState.data.storage_mode : null;
  const overallOnline = healthState.status === "success";

  return (
    <div>
      <div className="flex items-start justify-between px-8 py-6 border-b border-gray-800">
        <div>
          <h1 className="text-xl font-semibold text-white">Overview</h1>
          <p className="text-sm text-gray-400 mt-1">CloudTwin local runtime</p>
        </div>
        <div className="flex items-center gap-3 mt-1">
          {storageMode && <Badge label={`storage: ${storageMode}`} variant="gray" />}
          <div className="flex items-center gap-1.5">
            <span className={`w-2 h-2 rounded-full ${overallOnline ? "bg-emerald-400 animate-pulse" : "bg-red-400"}`} />
            <span className="text-sm text-gray-400">{overallOnline ? "online" : "offline"}</span>
          </div>
          <button
            onClick={refetch}
            className="text-xs text-gray-600 hover:text-gray-400 transition-colors"
          >
            ↻
          </button>
        </div>
      </div>

      {healthState.status === "loading" && <Spinner />}
      {healthState.status === "error" && (
        <ErrorBanner message={healthState.error} onRetry={refetch} />
      )}

      <div className="px-8 py-6">
        <h2 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-4">
          Services
        </h2>
        <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-3">
          {services.map((svc) => (
            <ServiceTile
              key={svc.to}
              {...svc}
              online={serviceStatus[svc.name.toLowerCase().replace(/ /g, "_")] ?? overallOnline}
            />
          ))}
        </div>

        {healthState.status === "success" && (
          <div className="mt-8">
            <h2 className="text-sm font-semibold text-gray-400 uppercase tracking-wider mb-4">
              Runtime Info
            </h2>
            <div className="bg-gray-900 border border-gray-800 rounded-xl divide-y divide-gray-800">
              {Object.entries(healthState.data.services).map(([key, value]) => (
                <div key={key} className="flex items-center justify-between px-5 py-3">
                  <span className="text-sm text-gray-300 font-mono">{key}</span>
                  <Badge label={value ? "enabled" : "disabled"} variant={value ? "green" : "gray"} />
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
