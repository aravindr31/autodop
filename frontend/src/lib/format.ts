/** Formatting + misc helpers for the AutoDOP UI. */

/** "2000" -> 2,000 (Indian digit grouping). */
export function formatINR(value: string | number): string {
  const n = typeof value === 'string' ? Number(value.replace(/[^\d]/g, '')) : value;
  if (Number.isNaN(n)) return String(value);
  return n.toLocaleString('en-IN');
}

/** "2,000" parlance used on cards. */
export function denominationLabel(value: string): string {
  const n = Number(value.replace(/[^\d]/g, ''));
  if (Number.isNaN(n) || n === 0) return value;
  return `₹ ${n.toLocaleString('en-IN')}`;
}

/** Random, collision-resistant id for client-created lists. */
export function newId(): string {
  const bytes = new Array(16) as number[];
  if (typeof crypto !== 'undefined' && crypto.randomUUID) return crypto.randomUUID();
  for (let i = 0; i < 16; i++) bytes[i] = Math.floor(Math.random() * 256);
  return Array.from(bytes).map((b) => b.toString(16).padStart(2, '0')).join('');
}

/** Case-insensitive substring search across the searchable account fields. */
export function matchesQuery(account: { Name: string; Number: string; CNumber: string; Ref_Number: string }, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return (
    account.Name.toLowerCase().includes(q) ||
    account.Number.toLowerCase().includes(q) ||
    account.CNumber.toLowerCase().includes(q) ||
    account.Ref_Number.toLowerCase().includes(q)
  );
}