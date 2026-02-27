interface EmptyStateProps {
  title?: string;
  message?: string;
}

export function EmptyState({
  title = "No resources",
  message = "Nothing here yet.",
}: EmptyStateProps) {
  return (
    <div className="flex flex-col items-center justify-center py-16 text-center">
      <div className="text-4xl mb-3 opacity-30">∅</div>
      <p className="text-gray-400 font-medium">{title}</p>
      <p className="text-gray-600 text-sm mt-1">{message}</p>
    </div>
  );
}
