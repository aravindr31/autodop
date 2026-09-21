import raw from '../data/accounts.json';
import type { Account } from './types';

/**
 * Seed dataset for development (spec §7: "The initial account data can be a
 * static JSON import"). The raw documents carry MongoDB's BSON form
 * (`_id: { $oid: ... }`) — normalize to a plain hex string key.
 *
 * This is the *seed* only: the live account list is owned by the store
 * (see `src/lib/store.ts`) so accounts can be added/deleted and persist.
 * Replace `src/data/accounts.json` with real data; the seed is applied on a
 * fresh (empty) store.
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

export const SEED_ACCOUNTS: Account[] = (raw as Record<string, unknown>[]).map(normalize);