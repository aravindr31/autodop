/**
 * AutoDOP store smoke test — runs the REAL src/lib/store.ts (bundled by
 * scripts/run-smoke.mjs) in Node and exercises every store action +
 * localStorage persistence. Two stages in one process:
 *
 *   stage 1  fresh storage -> mutate -> assert live state + persisted blob
 *   stage 2  fresh re-import of the store (cache-busting query) -> assert that
 *            the earlier state was restored from localStorage
 *
 * Exported as runSmoke(); invoked via `npm run smoke`.
 */
const BUNDLE = process.env.AUTODOP_SMOKE_DIR ?? new URL('./smoke-bundle', import.meta.url).pathname;

// ---- localStorage shim (in-memory) — must be set before the store is imported ----
const mem = {};
globalThis.localStorage = {
  getItem: (k) => (k in mem ? mem[k] : null),
  setItem: (k, v) => {
    mem[k] = String(v);
  },
  removeItem: (k) => {
    delete mem[k];
  },
};

export async function runSmoke() {
  let failures = 0;
  const check = (label, cond, detail = '') => {
    const ok = Boolean(cond);
    if (!ok) failures += 1;
    console.log(`${ok ? 'PASS' : 'FAIL'}  ${label}${detail ? `  [${detail}]` : ''}`);
  };

  const storeMod = await import(`${BUNDLE}/store.mjs`);
  const store = storeMod.useStore;
  // zustand swaps the whole state object on `set` — always read a fresh
  // snapshot. Actions (below) are stable references, so keep calling those.
  const live = () => store.getState();

  // ---- stage 1: fresh store, mutate, assert ----
  console.log('--- STAGE 1: fresh state, mutate ---');
  const s0 = live();
  check('fresh: one default list', s0.lists.length === 1, `lists=${s0.lists.length}`);
  check('fresh: default named "Main List"', s0.lists[0].name === 'Main List');
  check('fresh: active = main', s0.activeListId === s0.lists[0].id);
  check('fresh: totals 0/0', s0.totalsOf(s0.activeListId).count === 0 && s0.totalsOf(s0.activeListId).amount === 0);

  const s = live();
  const { id: vipId, name: vipName } = s.createList('VIP');
  check('create: VIP added', live().lists.length === 2, `n=${live().lists.length}`);
  check('create: active switched to VIP', live().activeListId === vipId);
  check('create: auto-name when blank', live().createList('  ').name.startsWith('List'));

  const { ACCOUNTS } = await import(`${BUNDLE}/accounts.mjs`);
  const acc = ACCOUNTS[0];
  const denom = Number(acc.Denomination.replace(/\D/g, ''));

  s.setActiveList(vipId);
  s.addToActive(acc._id);
  check('add: in list', live().lists.find((l) => l.id === vipId).accountIds.length === 1);
  check('add: isAdded true', live().isAdded(acc._id) === true);
  check('add: listNameOf = VIP', live().listNameOf(acc._id) === vipName, `got=${live().listNameOf(acc._id)}`);
  check('add: totals count=1', live().totalsOf(vipId).count === 1, `count=${live().totalsOf(vipId).count}`);
  check('add: totals amount=denom', live().totalsOf(vipId).amount === denom, `amount=${live().totalsOf(vipId).amount} denom=${denom}`);

  s.addToActive(acc._id); // duplicate -> must be a no-op
  check('noop: duplicate add ignored', live().lists.find((l) => l.id === vipId).accountIds.length === 1);

  const acc2 = ACCOUNTS.find((a) => a._id !== acc._id);
  s.renameList(vipId, 'VIP Club');
  s.setActiveList(live().lists.find((l) => l.id !== vipId).id);
  const mainId = live().activeListId;
  s.addToActive(acc2._id);
  check('multi: acc2 in main', live().listNameOf(acc2._id) !== '');
  check('multi: acc1 in VIP Club', live().listNameOf(acc._id) === 'VIP Club');

  s.removeFromList(mainId, acc2._id);
  check('remove: acc2 no longer in any list', live().isAdded(acc2._id) === false);

  s.clearList(vipId);
  check('clear: VIP empty', live().totalsOf(vipId).count === 0);

  check('endpoint: trimmed', (s.setSubmitEndpoint('  https://x  '), live().submitEndpoint) === 'https://x');
  s.setActiveList(vipId);
  s.addToActive(acc._id); // re-add so stage 2 has something to restore
  s.setSubmitEndpoint('https://api.example/v1/batch');
  check('endpoint: persisted value', live().submitEndpoint === 'https://api.example/v1/batch');

  // persistence blob (subscribe writes on every mutation)
  const blobTxt = mem['autodop-state-v1'] ?? '';
  const blob = JSON.parse(blobTxt);
  check('persist: blob written', Boolean(blobTxt));
  check('persist: version=1', blob.version === 1);
  check('persist: lists include VIP Club', blob.lists.some((l) => l.name === 'VIP Club'));
  check('persist: lists include Main List', blob.lists.some((l) => l.name === 'Main List'));
  check('persist: has 3 lists', blob.lists.length === 3, `n=${blob.lists.length}`);
  check('persist: active id saved', typeof blob.activeListId === 'string' && blob.activeListId.length > 0);
  check('persist: endpoint saved', blob.submitEndpoint === 'https://api.example/v1/batch');

  // ---- stage 2: fresh store instance => state restored from localStorage ----
  console.log('\n--- STAGE 2: reload persisted state ---');
  const s2 = (await import(`${BUNDLE}/store.mjs?t=${Date.now()}`)).useStore.getState();
  check('reload: 3 lists restored', s2.lists.length === 3, `n=${s2.lists.length}`);
  check('reload: VIP Club present', s2.lists.some((l) => l.name === 'VIP Club'));
  check('reload: active id restored (non-empty)', s2.activeListId.length > 0);
  check('reload: endpoint restored', s2.submitEndpoint === 'https://api.example/v1/batch');
  const vip = s2.lists.find((l) => l.name === 'VIP Club');
  check('reload: VIP Club has 1 account', vip && vip.accountIds.length === 1, `n=${vip?.accountIds.length}`);
  const restoredAcc = ACCOUNTS.find((a) => a._id === vip.accountIds[0]);
  check(
    'reload: recomputed totals match',
    s2.totalsOf(vip.id).count === 1 &&
      s2.totalsOf(vip.id).amount === Number(restoredAcc.Denomination.replace(/\D/g, '')),
  );
  check('reload: listNameOf resolves', s2.listNameOf(vip.accountIds[0]) === 'VIP Club');

  console.log(failures === 0 ? '\nSMOKE TEST: ALL GOOD' : `\n${failures} FAILURE(S)`);
  return failures === 0 ? 0 : 1;
}