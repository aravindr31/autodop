/**
 * Backend integration (spec §4.3): POST the account objects of a list to a
 * configurable endpoint and report success/failure back to the UI.
 */
import type { Account, AccountList } from './types';

export interface SubmitResult {
  ok: boolean;
  status?: number;
  /** Human-readable message for the UI to surface. */
  message: string;
}

/** Serializes an account into the exact document shape from spec §2. */
function toDocument(account: Account, listName: string): Record<string, unknown> {
  return {
    Number: account.Number,
    Name: account.Name,
    Denomination: account.Denomination,
    CNumber: account.CNumber,
    Ref_Number: account.Ref_Number,
    addedIn: listName,
    _id: { $oid: account._id },
  };
}

/**
 * Resolve an account by id. A resolver is injected rather than importing the
 * live store here so this module stays a pure library.
 */
export type AccountResolver = (id: string) => Account | undefined;

export async function submitList(
  list: AccountList,
  endpoint: string,
  accountOf: AccountResolver,
): Promise<SubmitResult> {
  const trimmed = (endpoint ?? '').trim();
  if (!trimmed) {
    return { ok: false, message: 'No backend endpoint configured yet.' };
  }

  const docs: Record<string, unknown>[] = [];
  for (const id of list.accountIds) {
    const acc = accountOf(id);
    if (acc) docs.push(toDocument(acc, list.name));
  }

  if (docs.length === 0) {
    return { ok: false, message: 'This list is empty — add accounts before submitting.' };
  }

  try {
    const res = await fetch(trimmed, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(docs),
    });
    if (res.ok) {
      return { ok: true, status: res.status, message: `Submitted ${docs.length} account(s).` };
    }
    return {
      ok: false,
      status: res.status,
      message: `Backend responded ${res.status} ${res.statusText}.`,
    };
  } catch (err) {
    return {
      ok: false,
      message: `Request failed: ${err instanceof Error ? err.message : String(err)}`,
    };
  }
}