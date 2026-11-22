// Infrared remote (full-size SMHUB): learn a button from any remote, keep it
// under a name and a device, send it from here, from the home page, from
// Home Assistant or a script. Learning shows the code the moment it arrives.
import { useEffect, useState } from 'preact/hooks';
import { Download, Pencil, Radar, Send, Star, Trash, Tv, Upload, X } from 'lucide-preact';
import { api, apiUrl } from '../../api/client';
import type { Ir, IrCode } from '../../api/types';
import { t } from '../../text/i18n';
import { compare, relative } from '../../text/format';
import { confirm } from '../../state/confirm';
import { sendCode } from '../../state/hardware';
import { advanced } from '../../state/prefs';
import { errorText, save, useFieldErrors } from '../../state/save';
import { setLive, useLive } from '../../state/store';
import { toast } from '../../state/toasts';
import { Button, IconButton, LinkButton } from '../../kit/Button';
import { CopyButton } from '../../kit/Bits';
import { Dialog } from '../../kit/Dialog';
import { Banner, EmptyState, Loading } from '../../kit/Feedback';
import { SelectField, Switch, TextArea, TextField } from '../../kit/Field';
import { Actions, Page, Rows, Section } from '../../kit/Page';
import { Status } from '../../kit/Status';
import c from '../common.module.css';
import s from './Infrared.module.css';

function codeText(protocol: string | null, scancode: string | null, edges: number): string {
  return protocol && scancode ? `${protocol.toUpperCase()} ${scancode}` : t('ir.raw', { count: edges });
}

function Learn({ ir, devices }: { ir: Ir; devices: string[] }) {
  const [name, setName] = useState('');
  const [device, setDevice] = useState('');
  const [busy, setBusy] = useState(false);
  const errors = useFieldErrors();
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (!ir.learning) return;
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [ir.learning]);
  const start = async () => {
    try {
      setLive('ir', await api.post('/ir/learn'));
    } catch (e) {
      toast({ text: errorText(e), tone: 'danger' });
    }
  };
  const cancel = async () => setLive('ir', await api.del('/ir/learn'));
  const keep = async (e: Event) => {
    e.preventDefault();
    if (!name.trim()) return;
    setBusy(true);
    const r = await save({ send: () => api.post('/ir/codes', { body: { name: name.trim(), device: device.trim(), from_last: true } }), errors, success: t('ir.saved', { name: name.trim() }) });
    setBusy(false);
    if (r) setName('');
  };
  const left = ir.learning ? Math.max(0, Math.round((Date.parse(ir.learning.until) - now) / 1000)) : 0;
  const last = ir.last && !ir.last.matched && ir.last.edges > 0 ? ir.last : null;
  return (
    <Section id="learn" title={t('ir.learn')} icon={Radar} description={t('ir.learnDesc')}>
      {ir.learning ? (
        <div class={s.learning} aria-live="polite">
          <span class={s.pulse} aria-hidden="true" />
          <p>
            <strong>{t('ir.pressNow')}</strong>
            <br />
            <span class="muted">{t('ir.secondsLeft', { count: left })}</span>
          </p>
          <Button icon={X} onClick={() => void cancel()}>
            {t('common.cancel')}
          </Button>
        </div>
      ) : (
        <Actions>
          <Button variant="primary" icon={Radar} onClick={() => void start()}>
            {last ? t('ir.learnAgain') : t('ir.learnButton')}
          </Button>
        </Actions>
      )}
      {last && !ir.learning && (
        <form class={c.form} onSubmit={keep} noValidate>
          <p aria-live="polite">
            <Status tone="ok" size="sm">
              {t('ir.received', { code: codeText(last.protocol, last.scancode, last.edges) })}
            </Status>
          </p>
          <TextField label={t('ir.name')} hint={t('ir.nameHint')} value={name} onValue={setName} maxLength={40} required error={errors.value.name} autoFocus />
          <TextField label={t('ir.device')} hint={t('ir.deviceHint')} value={device} onValue={setDevice} maxLength={40} list="ir-devices" error={errors.value.device} />
          <datalist id="ir-devices">
            {devices.map((d) => (
              <option key={d} value={d} />
            ))}
          </datalist>
          <Actions>
            <Button type="submit" variant="primary" busy={busy} disabled={!name.trim()}>
              {t('ir.keep')}
            </Button>
          </Actions>
        </form>
      )}
    </Section>
  );
}

function RenameDialog({ code, devices, onClose }: { code: IrCode | null; devices: string[]; onClose: () => void }) {
  const [name, setName] = useState(code?.name ?? '');
  const [device, setDevice] = useState(code?.device ?? '');
  const errors = useFieldErrors();
  useEffect(() => {
    setName(code?.name ?? '');
    setDevice(code?.device ?? '');
  }, [code]);
  const submit = async () => {
    if (!code) return;
    const r = await save({ send: () => api.patch('/ir/codes/{id}', { path: { id: code.id }, body: { name: name.trim(), device: device.trim() } }), errors, success: t('common.saved') });
    if (r) onClose();
  };
  return (
    <Dialog
      open={!!code}
      onClose={onClose}
      onSubmit={submit}
      title={t('ir.rename')}
      size="sm"
      footer={
        <>
          <Button onClick={onClose}>{t('common.cancel')}</Button>
          <Button type="submit" variant="primary">
            {t('common.save')}
          </Button>
        </>
      }
    >
      <TextField label={t('ir.name')} value={name} onValue={setName} maxLength={40} required error={errors.value.name} autoFocus />
      <TextField label={t('ir.device')} value={device} onValue={setDevice} maxLength={40} list="ir-devices-rename" error={errors.value.device} />
      <datalist id="ir-devices-rename">
        {devices.map((d) => (
          <option key={d} value={d} />
        ))}
      </datalist>
    </Dialog>
  );
}

function Codes({ ir, devices, onRename }: { ir: Ir; devices: string[]; onRename: (c: IrCode) => void }) {
  const remove = async (code: IrCode) => {
    if (!(await confirm({ title: t('ir.deleteTitle', { name: code.name }), body: t('ir.deleteBody'), confirm: t('ir.delete'), tone: 'danger' }))) return;
    try {
      await api.del('/ir/codes/{id}', { path: { id: code.id } });
      toast({ text: t('ir.deleted', { name: code.name }), tone: 'ok' });
    } catch (e) {
      toast({ text: errorText(e), tone: 'danger' });
    }
  };
  const star = async (code: IrCode) => {
    try {
      await api.patch('/ir/codes/{id}', { path: { id: code.id }, body: { favourite: !code.favourite } });
    } catch (e) {
      toast({ text: errorText(e), tone: 'danger' });
    }
  };
  if (ir.codes.length === 0) {
    return (
      <Section id="codes" title={t('ir.codes')} icon={Tv}>
        <EmptyState icon={Tv} title={t('ir.none')}>
          {t('ir.noneHint')}
        </EmptyState>
      </Section>
    );
  }
  return (
    <Section id="codes" title={t('ir.codes')} icon={Tv} description={t('ir.codesDesc', { count: ir.codes.length, max: ir.max_codes })}>
      {devices.map((d) => (
        <div key={d || '-'} class={s.device}>
          <h3 class={s.deviceTitle}>{d || t('ir.noDevice')}</h3>
          <ul class={c.list}>
            {ir.codes
              .filter((x) => x.device === d)
              .map((code) => (
                <li key={code.id} class={c.item}>
                  <div class={c.itemMain}>
                    <span class={c.itemTitle}>{code.name}</span>
                    {advanced.value && <span class={`${c.itemMeta} mono`}>{codeText(code.protocol, code.scancode, code.edges)}</span>}
                  </div>
                  <span class={c.itemActions}>
                    <Button size="sm" variant="primary" icon={Send} onClick={() => void sendCode(code)}>
                      {t('ir.send')}
                    </Button>
                    <IconButton icon={Star} label={code.favourite ? t('ir.unfavourite', { name: code.name }) : t('ir.favourite', { name: code.name })} aria-pressed={code.favourite} onClick={() => void star(code)} class={code.favourite ? s.starred : undefined} />
                    <IconButton icon={Pencil} label={t('ir.renameNamed', { name: code.name })} onClick={() => onRename(code)} />
                    <IconButton icon={Trash} label={t('ir.deleteNamed', { name: code.name })} onClick={() => void remove(code)} />
                  </span>
                </li>
              ))}
          </ul>
        </div>
      ))}
    </Section>
  );
}

function HomeAssistant({ ir }: { ir: Ir }) {
  const [id, setId] = useState(ir.codes[0]?.id ?? '');
  const code = ir.codes.find((x) => x.id === id) ?? ir.codes[0];
  if (!code) return null;
  const key = (code.device ? `${code.device}_${code.name}` : code.name).toLowerCase().replace(/[^a-z0-9]+/g, '_').replace(/^_|_$/g, '') || 'ir_code';
  const yaml = `# secrets.yaml: smhub_bearer: "Bearer <token>"\nrest_command:\n  ${key}:\n    url: http://${location.host}/api/v2/ir/codes/${code.id}/send\n    method: post\n    headers:\n      Authorization: !secret smhub_bearer`;
  return (
    <Section id="ha" title={t('ir.ha')} description={t('ir.haDesc')}>
      <SelectField label={t('ir.code')} value={code.id} onValue={setId} options={ir.codes.map((x) => ({ value: x.id, label: x.device ? `${x.device} · ${x.name}` : x.name }))} />
      <pre class={s.snippet}>{yaml}</pre>
      <Actions>
        <CopyButton text={yaml} label={t('ir.copyHa')} />
        <LinkButton href="/settings/accounts#tokens" size="sm" variant="ghost">
          {t('ir.makeToken')}
        </LinkButton>
      </Actions>
    </Section>
  );
}

function Expert({ ir, settingsListen }: { ir: Ir; settingsListen: (v: boolean) => void }) {
  const [protocol, setProtocol] = useState('nec');
  const [scancode, setScancode] = useState('');
  const [importText, setImportText] = useState('');
  const sendSpec = async () => {
    try {
      await api.post('/ir/send', { body: { protocol, scancode: scancode.trim() } });
      toast({ text: t('ir.sentRaw'), tone: 'ok' });
    } catch (e) {
      toast({ text: errorText(e), tone: 'danger' });
    }
  };
  const doImport = async () => {
    try {
      const data = JSON.parse(importText) as { codes?: unknown[] };
      const r = await api.post('/ir/import', { body: { codes: (data.codes ?? []) as Record<string, unknown>[] } });
      toast({ text: t('ir.imported', { count: r.imported }), tone: 'ok' });
      setImportText('');
    } catch (e) {
      toast({ text: e instanceof SyntaxError ? t('ir.importBad') : errorText(e), tone: 'danger' });
    }
  };
  const protocols = ['nec', 'necx', 'nec32', 'rc5', 'rc5x_20', 'rc6_0', 'rc6_mce', 'jvc', 'sony12', 'sony15', 'sony20', 'sanyo', 'sharp', 'rcmm12', 'rcmm24', 'rcmm32'];
  return (
    <Section id="expert" title={t('ir.expert')}>
      <Rows>
        <Switch label={t('ir.listen')} description={t('ir.listenDesc')} checked={ir.listening} onChange={settingsListen} />
      </Rows>
      {ir.listening && ir.last && (
        <p aria-live="polite" class="muted">
          {t('ir.lastHeard', { code: codeText(ir.last.protocol, ir.last.scancode, ir.last.edges), when: relative(ir.last.at) })}
          {ir.last.matched && ` · ${ir.last.matched.device ? `${ir.last.matched.device} · ` : ''}${ir.last.matched.name}`}
        </p>
      )}
      <div class={s.spec}>
        <SelectField label={t('ir.protocol')} value={protocol} onValue={setProtocol} options={protocols.map((p) => ({ value: p, label: p.toUpperCase() }))} />
        <TextField label={t('ir.scancode')} hint={t('ir.scancodeHint')} value={scancode} onValue={setScancode} mono />
      </div>
      <Actions>
        <Button icon={Send} disabled={!scancode.trim()} onClick={() => void sendSpec()}>
          {t('ir.sendCode')}
        </Button>
      </Actions>
      <Actions>
        <LinkButton href={apiUrl('/ir/export')} download="smhub-ir-codes.json" icon={Download} size="sm">
          {t('ir.export')}
        </LinkButton>
      </Actions>
      <TextArea label={t('ir.import')} hint={t('ir.importHint')} value={importText} onValue={setImportText} rows={3} mono />
      <Actions>
        <Button icon={Upload} disabled={!importText.trim()} onClick={() => void doImport()}>
          {t('ir.importButton')}
        </Button>
      </Actions>
    </Section>
  );
}

export default function Infrared() {
  const ir = useLive('ir');
  const [renaming, setRenaming] = useState<IrCode | null>(null);
  const devices = ir ? [...new Set(ir.codes.map((x) => x.device))].sort(compare) : [];
  const listen = (v: boolean) => void save({ send: () => api.patch('/settings/hardware', { body: { ir: { listen: v } } }), success: t('common.saved') });
  return (
    <Page title={t('nav.infrared')} back={{ href: '/peripherals', label: t('nav.peripherals') }} subtitle={t('nav.infraredDesc')} narrow>
      {!ir ? (
        <Loading lines={6} />
      ) : !ir.available ? (
        <Banner tone="info">{t('ir.unavailable')}</Banner>
      ) : (
        <>
          <Learn ir={ir} devices={devices} />
          <Codes ir={ir} devices={devices} onRename={setRenaming} />
          <HomeAssistant ir={ir} />
          {advanced.value && <Expert ir={ir} settingsListen={listen} />}
        </>
      )}
      <RenameDialog code={renaming} devices={devices} onClose={() => setRenaming(null)} />
    </Page>
  );
}
