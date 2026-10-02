/** Formatting + misc helpers for the AutoDOP UI. */

/** "2,000" / "2000.00" -> 2,000 (Indian digit grouping, decimals dropped). */
export function formatINR(value: string | number): string {
  // Strip everything but digits and the decimal point; a stray "10000.00"
  // must not become 1000000 by swallowing the dot.
  const n = Number(typeof value === 'string' ? value.replace(/[^\d.]/g, '') : value);
  if (Number.isNaN(n)) return String(value);
  return Math.round(n).toLocaleString('en-IN');
}

/** "2,000" parlance used on cards. */
export function denominationLabel(value: string): string {
  const n = Number(value.replace(/[^\d.]/g, ''));
  if (Number.isNaN(n) || n === 0) return value;
  return `₹ ${Math.round(n).toLocaleString('en-IN')}`;
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

/** 1-based bijective base-26: 1 -> A, 26 -> Z, 27 -> AA, 28 -> AB … */
function toLetter(n: number): string {
  let s = '';
  while (n > 0) {
    const rem = (n - 1) % 26;
    s = String.fromCharCode(65 + rem) + s;
    n = Math.floor((n - 1) / 26);
  }
  return s;
}

/**
 * Next auto-generated list label, respecting names already in use so lists
 * come out alphabetical (A, B, C … Z, AA, AB …). Skips any exact case-insensitive
 * collision; falls through to multi-letter after a full A–Z run.
 */
export function nextListLabel(existing: string[]): string {
  const used = new Set(existing.map((n) => n.trim().toUpperCase()));
  for (let n = 1; ; n++) {
    const label = toLetter(n);
    if (!used.has(label)) return label;
  }
}