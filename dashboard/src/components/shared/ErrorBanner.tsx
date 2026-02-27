interface ErrorBannerProps {
  message: string;
  onRetry?: () => void;
}

export function ErrorBanner({ message, onRetry }: ErrorBannerProps) {
  // 404/not-yet-implemented is expected while the API endpoints are being built
  const isPending = message.includes("404") || message.includes("Not Found");

  return (
    <div className={`mx-8 mt-6 px-4 py-3 rounded-lg border text-sm flex items-start gap-3 ${
      isPending
        ? "bg-yellow-500/10 border-yellow-500/20 text-yellow-300"
        : "bg-red-500/10 border-red-500/20 text-red-300"
    }`}>
      <span className="mt-0.5">{isPending ? "⏳" : "⚠"}</span>
      <div className="flex-1">
        {isPending
          ? "API endpoint not yet available — backend implementation pending."
          : message}
      </div>
      {onRetry && !isPending && (
        <button
          onClick={onRetry}
          className="text-xs underline hover:no-underline shrink-0"
        >
          Retry
        </button>
      )}
    </div>
  );
}
