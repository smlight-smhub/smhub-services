// Long operations (installs, OS updates, flashing, restores). Their progress
// arrives on the event connection; the shell's tray shows them on every page.
import { computed, effect } from '@preact/signals';
import type { Operation, OperationKind } from '../api/types';
import { t, tx } from '../text/i18n';
import { appTitle } from '../text/names';
import { errorText } from './save';
import { toast } from './toasts';
import { live, upsertOperation } from './store';

export const operations = computed(() => live('operations').value?.operations ?? []);
export const runningOps = computed(() => operations.value.filter((o) => o.state === 'running'));

export function isOperation(x: unknown): x is Operation {
  return !!x && typeof x === 'object' && 'kind' in x && 'state' in x && 'started_at' in x && 'id' in x;
}

/** Sends a command that answers 202 with an operation, and shows it at once. */
export async function startOperation<A>(send: () => Promise<A>): Promise<Operation | undefined> {
  try {
    const answer = await send();
    if (isOperation(answer)) {
      upsertOperation(answer);
      return answer;
    }
    return undefined;
  } catch (e) {
    toast({ text: t('op.couldNotStart', { reason: errorText(e) }), tone: 'danger' });
    return undefined;
  }
}

/** The newest operation of these kinds for this target. */
export function latestOperation(kinds: OperationKind[], target?: string | null): Operation | undefined {
  return operations.value.find((o) => kinds.includes(o.kind) && (target === undefined || o.target === target));
}

export function operationById(id: string | undefined): Operation | undefined {
  return id ? operations.value.find((o) => o.id === id) : undefined;
}

/** "Installing Zigbee2MQTT", "Updating the hub", ... */
export function operationTitle(op: Operation): string {
  const raw = op.target ?? '';
  const target = op.kind.startsWith('app_') ? appTitle(raw) : raw;
  const done = op.state !== 'running';
  const key = `op.${op.kind}.${done ? (op.state === 'succeeded' ? 'done' : 'failed') : 'running'}`;
  return tx(key, op.kind, { target });
}

/** The current step in words: a translation key from the hub, or its English text. */
export function operationStep(op: Operation): string | undefined {
  if (op.state === 'failed') return op.error ?? undefined;
  if (!op.step) return undefined;
  return tx(`step.${op.step}`, op.step);
}

// Tell the user when an operation ends, wherever they are.
const seen = new Map<string, Operation['state']>();
let primed = false;
effect(() => {
  const ops = operations.value;
  for (const op of ops) {
    const prev = seen.get(op.id);
    seen.set(op.id, op.state);
    if (!primed || prev !== 'running' || op.state === 'running') continue;
    toast({ text: operationTitle(op), tone: op.state === 'succeeded' ? 'ok' : 'danger' });
  }
  if (ops.length || live('operations').value) primed = true;
  if (seen.size > 100) for (const k of [...seen.keys()].slice(0, seen.size - 100)) seen.delete(k);
});
