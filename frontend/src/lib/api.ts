import type { Account, AccountList } from './types';

export interface SubmitResult {
  ok: boolean;
  status?: number;

  message: string;
}


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
