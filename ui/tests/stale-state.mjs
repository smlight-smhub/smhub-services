// Does the live connection end on the right state after a service is stopped or started while
// other requests keep the hub's status refresh busy?
//   node tests/stale-state.mjs [hub URL] [service] [rounds]
const hub = process.argv[2] ?? process.env.SMHUB_URL ?? 'http://127.0.0.1:8000';
const svc = process.argv[3] ?? 'ntpd';
const rounds = Number(process.argv[4] ?? 10);
const H = { 'Content-Type': 'application/json', Origin: hub };
let last = null;
const ws = new WebSocket(hub.replace('http', 'ws') + '/ws/v2/events');
await new Promise((r) => ws.addEventListener('open', r));
ws.addEventListener('message', (e) => {
  const m = JSON.parse(e.data);
  if (m.type === 'state' && m.resource === 'services') last = m.data.services.find((s) => s.name === svc)?.state ?? last;
});
ws.send(JSON.stringify({ op: 'sub', resources: ['services', 'apps'] }));
// keep the status refresh busy, as an open app page and the home page do
let busy = true;
const load = (async () => {
  while (busy) {
    await Promise.all([fetch(`${hub}/api/v2/apps`), fetch(`${hub}/api/v2/services`)]).catch(() => {});
  }
})();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let stale = 0;
for (let i = 0; i < rounds; i++) {
  for (const action of ['stop', 'start']) {
    await fetch(`${hub}/api/v2/services/${svc}/${action}`, { method: 'POST', headers: H });
    await sleep(6000); // well past the 2 s status cache and the watcher's 0.4 s delay
    const real = (await (await fetch(`${hub}/api/v2/services`)).json()).services.find((s) => s.name === svc)?.state;
    const want = action === 'stop' ? 'stopped' : 'started';
    const ok = last === real && real === want;
    if (!ok) stale++;
    console.log(`${i + 1} ${action}: hub ${real}, live connection says ${last}${ok ? '' : '   <-- stale'}`);
  }
}
busy = false;
await load;
ws.close();
console.log(`${stale} of ${rounds * 2} ended stale`);
