interface StatCardProps {
  label: string;
  value: number | string;
  accent?: string; // tailwind text-color class
}

export function StatCard({ label, value, accent = "text-white" }: StatCardProps) {
  return (
    <div className="bg-gray-900 border border-gray-800 rounded-xl px-5 py-4">
      <p className="text-xs text-gray-500 uppercase tracking-wide">{label}</p>
      <p className={`text-3xl font-semibold mt-1 ${accent}`}>{value}</p>
    </div>
  );
}
