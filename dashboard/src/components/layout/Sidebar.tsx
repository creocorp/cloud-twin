import { NavLink } from "react-router-dom";

interface NavItem {
  to: string;
  label: string;
}

interface NavGroup {
  label: string;
  color: string;
  dot: string;
  items: NavItem[];
}

const nav: NavGroup[] = [
  {
    label: "AWS",
    color: "text-orange-400",
    dot: "bg-orange-400",
    items: [
      { to: "/aws/ses", label: "SES" },
      { to: "/aws/s3", label: "S3" },
      { to: "/aws/sns", label: "SNS" },
      { to: "/aws/sqs", label: "SQS" },
    ],
  },
  {
    label: "Azure",
    color: "text-blue-400",
    dot: "bg-blue-400",
    items: [
      { to: "/azure/blob", label: "Blob Storage" },
      { to: "/azure/servicebus", label: "Service Bus" },
    ],
  },
  {
    label: "GCP",
    color: "text-sky-400",
    dot: "bg-sky-400",
    items: [
      { to: "/gcp/storage", label: "Cloud Storage" },
      { to: "/gcp/pubsub", label: "Pub/Sub" },
    ],
  },
];

export function Sidebar() {
  return (
    <aside className="w-56 min-h-screen bg-gray-900 border-r border-gray-800 flex flex-col">
      {/* Logo */}
      <div className="px-5 py-4 border-b border-gray-800">
        <NavLink to="/" className="flex items-center gap-2">
          <span className="text-white font-semibold text-sm tracking-wide">
            ☁ CloudTwin
          </span>
        </NavLink>
        <p className="text-gray-500 text-xs mt-0.5">local runtime</p>
      </div>

      {/* Overview */}
      <div className="px-3 pt-4 pb-2">
        <NavLink
          to="/"
          end
          className={({ isActive }) =>
            `flex items-center gap-2 px-3 py-2 rounded-md text-sm transition-colors ${
              isActive
                ? "bg-gray-700 text-white"
                : "text-gray-400 hover:text-white hover:bg-gray-800"
            }`
          }
        >
          Overview
        </NavLink>
        <NavLink
          to="/events"
          className={({ isActive }) =>
            `flex items-center gap-2 px-3 py-2 rounded-md text-sm transition-colors ${
              isActive
                ? "bg-gray-700 text-white"
                : "text-gray-400 hover:text-white hover:bg-gray-800"
            }`
          }
        >
          Event Log
        </NavLink>
      </div>

      {/* Provider groups */}
      {nav.map((group) => (
        <div key={group.label} className="px-3 py-3">
          <div className={`flex items-center gap-1.5 px-3 mb-1`}>
            <span className={`w-2 h-2 rounded-full ${group.dot}`} />
            <span className={`text-xs font-semibold uppercase tracking-wider ${group.color}`}>
              {group.label}
            </span>
          </div>
          {group.items.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              className={({ isActive }) =>
                `flex items-center gap-2 px-3 py-2 rounded-md text-sm transition-colors ${
                  isActive
                    ? "bg-gray-700 text-white"
                    : "text-gray-400 hover:text-white hover:bg-gray-800"
                }`
              }
            >
              {item.label}
            </NavLink>
          ))}
        </div>
      ))}

      <div className="mt-auto px-5 py-4 border-t border-gray-800">
        <p className="text-gray-600 text-xs">port 4793 · 8793</p>
      </div>
    </aside>
  );
}
