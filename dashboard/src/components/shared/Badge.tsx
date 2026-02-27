type BadgeVariant = "green" | "red" | "yellow" | "blue" | "gray";

const styles: Record<BadgeVariant, string> = {
  green: "bg-emerald-500/15 text-emerald-400",
  red: "bg-red-500/15 text-red-400",
  yellow: "bg-yellow-500/15 text-yellow-400",
  blue: "bg-blue-500/15 text-blue-400",
  gray: "bg-gray-500/15 text-gray-400",
};

interface BadgeProps {
  label: string;
  variant?: BadgeVariant;
}

export function Badge({ label, variant = "gray" }: BadgeProps) {
  return (
    <span className={`inline-flex items-center px-2 py-0.5 rounded text-xs font-medium ${styles[variant]}`}>
      {label}
    </span>
  );
}
