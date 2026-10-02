import raw from '../data/accounts.json';
import type { Account } from './types';











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
