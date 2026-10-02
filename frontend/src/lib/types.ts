export interface Account {

  _id: string;
  Number: string;
  Name: string;

  Denomination: string;
  CNumber: string;
  Ref_Number: string;

  addedIn: string;
}


export interface NewAccountInput {
  Number: string;
  Name: string;
  Denomination: string;
  CNumber: string;
  Ref_Number: string;
}


export interface AccountList {
  id: string;
  name: string;

  accountIds: string[];







  rebates?: Record<string, number>;
}


export interface AuthCredential {
  salt: string;
  hash: string;
}


export interface PersistedState {
  accounts: Account[];
  lists: AccountList[];
  activeListId: string;
  submitEndpoint: string;
  auth: AuthCredential | null;
}
