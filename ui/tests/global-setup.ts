// Signs in once (when the hub asks for it) and keeps the session cookie for
// every test. On an open hub the stored state is simply empty.
import { request, type FullConfig } from '@playwright/test';
import { mkdirSync } from 'node:fs';

export default async function globalSetup(config: FullConfig) {
  const baseURL = config.projects[0]!.use.baseURL!;
  mkdirSync('test-results', { recursive: true });
  const ctx = await request.newContext({ baseURL });
  // The preview server may still be starting when this runs.
  let session = await ctx.get('/api/v2/session').catch(() => null);
  for (let i = 0; i < 30 && !session; i++) {
    await new Promise((r) => setTimeout(r, 1000));
    session = await ctx.get('/api/v2/session').catch(() => null);
  }
  if (session?.status() === 401) {
    const username = process.env.SMHUB_USER;
    const password = process.env.SMHUB_PASSWORD;
    if (!username || !password) throw new Error('The hub asks for sign-in: set SMHUB_USER and SMHUB_PASSWORD.');
    const res = await ctx.post('/api/v2/session', { data: { username, password } });
    if (!res.ok()) throw new Error(`Sign-in failed: ${res.status()} ${await res.text()}`);
  } else if (!session?.ok()) {
    throw new Error(`The hub does not answer GET /api/v2/session at ${baseURL}`);
  }
  await ctx.storageState({ path: 'test-results/.auth.json' });
  await ctx.dispose();
}
