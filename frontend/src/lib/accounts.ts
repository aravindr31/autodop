import raw from '../data/accounts.json';
import type { Account } from './types';

/**
 * Loads the development dataset (spec §7: "The initial account data can be a
 * static JSON import"). The raw documents carry MongoDB's BSON form
 * (`_id: { $oid: ... }`) — normalize to a plain hex string key.
 */
function normalize(row: Record<string, unknown>): Account {
  const _id = row._id as unknown;
  const id =
    typeof _id === 'string'
      ? _id
      : typeof _id === 'object' && _id && typeof (_id as { $oid?: unknown }).$oid === 'string'
        ? (row._id as { $oid: string }).$oid
        : '';
  return {
    _id: id,
    Number: String(row.Number ?? ''),
    Name: String(row.Name ?? ''),
    Denomination: String(row.Denomination ?? '0'),
    CNumber: String(row.CNumber ?? ''),
    Ref_Number: String(row.Ref_Number ?? ''),
    addedIn: String(row.addedIn ?? ''),
  };
}

export const ACCOUNTS: Account[] = (raw as Record<string, unknown>[]).map(normalize);

const byId = new Map<string, Account>();
for (const a of ACCOUNTS) byId.set(a._id, a);

export function accountById(id: string): Account | undefined {
  return byId.get(id);
}

export const TOTAL_ACCOUNTS: number = ACCOUNTS.length;