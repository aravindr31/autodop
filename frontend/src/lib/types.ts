/**
 * Core domain types for AutoDOP.
 * The account shape mirrors the MongoDB document from the Python backend:
 * https://iluvmarkets.github.io — see spec.md §2.
 */

/** A single bank/DOP account document (MongoDB projection). */
export interface Account {
  /** ObjectId serialized as hex string (e.g. "67822e54900a10d40ce71722"). */
  _id: string;
  Number: string;
  Name: string;
  /** Monthly denomination — kept as a string to match the source document. */
  Denomination: string;
  CNumber: string;
  Ref_Number: string;
  /** Name of the list this account was added to ("" when unassigned). */
  addedIn: string;
}

/** Fields the user supplies when creating a new account. */
export interface NewAccountInput {
  Number: string;
  Name: string;
  Denomination: string;
  CNumber: string;
  Ref_Number: string;
}

/** A user-defined, persistent list of account references. */
export interface AccountList {
  id: string;
  name: string;
  /** ObjectIds of the `Account` entries in this list (order preserved). */
  accountIds: string[];
}

/** Client-side session credential (salted SHA-256). Pending the real backend. */
export interface AuthCredential {
  salt: string;
  hash: string;
}

/** Subset of app state persisted to localStorage. */
export interface PersistedState {
  accounts: Account[];
  lists: AccountList[];
  activeListId: string;
  submitEndpoint: string;
  auth: AuthCredential | null;
}