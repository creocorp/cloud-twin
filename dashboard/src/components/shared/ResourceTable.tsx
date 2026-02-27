import { ReactNode } from "react";

export interface Column<T> {
  header: string;
  render: (row: T) => ReactNode;
  className?: string;
}

interface ResourceTableProps<T> {
  columns: Column<T>[];
  rows: T[];
  keyFn: (row: T) => string;
}

export function ResourceTable<T>({ columns, rows, keyFn }: ResourceTableProps<T>) {
  return (
    <div className="overflow-x-auto">
      <table className="w-full text-sm">
        <thead>
          <tr className="border-b border-gray-800">
            {columns.map((col) => (
              <th
                key={col.header}
                className={`px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider ${col.className ?? ""}`}
              >
                {col.header}
              </th>
            ))}
          </tr>
        </thead>
        <tbody className="divide-y divide-gray-800/60">
          {rows.map((row) => (
            <tr key={keyFn(row)} className="hover:bg-gray-800/40 transition-colors">
              {columns.map((col) => (
                <td
                  key={col.header}
                  className={`px-4 py-3 text-gray-300 ${col.className ?? ""}`}
                >
                  {col.render(row)}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
