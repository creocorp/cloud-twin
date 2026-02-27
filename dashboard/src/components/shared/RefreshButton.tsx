interface RefreshButtonProps {
  onClick: () => void;
}

export function RefreshButton({ onClick }: RefreshButtonProps) {
  return (
    <button
      onClick={onClick}
      className="flex items-center gap-1.5 px-3 py-1.5 rounded-md text-sm text-gray-400 bg-gray-800 hover:bg-gray-700 hover:text-white transition-colors"
    >
      ↻ Refresh
    </button>
  );
}
