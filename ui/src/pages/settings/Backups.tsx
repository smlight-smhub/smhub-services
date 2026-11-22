// Backups: create and download (a plain form POST, so the browser downloads
// natively and nothing is held in memory), restore by uploading in pieces
// with a check of the contents first, and the factory backup.
import { useRef, useState } from 'preact/hooks';
import { Archive, Download, FileArchive, History, Trash, Upload } from 'lucide-preact';
import { api, apiUrl, API_BASE } from '../../api/client';
import type { BackupInfo, BackupSettings, FactoryBackup } from '../../api/types';
import { uploadFile } from '../../api/upload';
import { t } from '../../text/i18n';
import { bytes, dateTime, list, percent } from '../../text/format';
import { confirm } from '../../state/confirm';
import { useGet } from '../../state/get';
import { startOperation } from '../../state/operations';
import { advanced } from '../../state/prefs';
import { errorText, save } from '../../state/save';
import { toast } from '../../state/toasts';
import { Button, LinkButton } from '../../kit/Button';
import { appTitle } from '../../kit/Bits';
import { Dialog } from '../../kit/Dialog';
import { Banner, ErrorState, Loading, ProgressBar } from '../../kit/Feedback';
import { ListField, Switch } from '../../kit/Field';
import { Facts, Page, Section } from '../../kit/Page';
import c from '../common.module.css';
import s from './Settings.module.css';

function CreateBackup() {
  const { data, error, reload, sig } = useGet('/settings/backups');
  const form = useRef<HTMLFormElement>(null);
  const [preparing, setPreparing] = useState(false);
  const [dirs, setDirs] = useState<string[] | null>(null);
  const extra = dirs ?? data?.extra_dirs ?? [];
  const patch = (body: BackupSettings) => save<BackupSettings>({ target: sig, optimistic: (b) => ({ ...b, ...body }), send: () => api.patch('/settings/backups', { body }), success: t('common.saved') });
  const create = () => {
    setPreparing(true);
    form.current?.submit();
    // The browser shows its own download when the archive is ready.
    setTimeout(() => setPreparing(false), 8000);
  };
  return (
    <Section id="create" title={t('backups.create')} icon={Archive} description={t('backups.createDesc')}>
      {error && !data ? (
        <ErrorState error={error} retry={reload} />
      ) : !data ? (
        <Loading />
      ) : (
        <>
          <Switch label={t('backups.includeHome')} description={t('backups.includeHomeDesc')} checked={!!data.include_home} onChange={(v) => void patch({ include_home: v })} />
          {advanced.value && (
            <>
              <ListField label={t('backups.extraDirs')} hint={t('backups.extraDirsHint')} placeholder="/mnt/user/data" value={extra} onValue={setDirs} />
              {dirs && (
                <div>
                  <Button
                    size="sm"
                    onClick={async () => {
                      if (await patch({ extra_dirs: dirs.filter(Boolean) })) setDirs(null);
                    }}
                  >
                    {t('backups.saveDirs')}
                  </Button>
                </div>
              )}
            </>
          )}
          <form ref={form} method="post" action={`${API_BASE}/backups/archive`} target="backup-download" class={s.hiddenFrame}>
            <input type="hidden" name="include_home" value={String(!!data.include_home)} />
            <input type="hidden" name="extra_dirs" value={extra.filter(Boolean).join(',')} />
          </form>
          <iframe
            name="backup-download"
            title={t('backups.download')}
            class={s.hiddenFrame}
            onLoad={(e) => {
              // A download does not load the frame; an error answer does.
              try {
                const text = (e.currentTarget as HTMLIFrameElement).contentDocument?.body?.textContent ?? '';
                if (!text) return;
                const err = JSON.parse(text) as { error?: { message?: string } };
                setPreparing(false);
                toast({ text: t('backups.failed', { reason: err.error?.message ?? text }), tone: 'danger' });
              } catch {
                /* not readable, or not an error */
              }
            }}
          />
          <Banner tone="info">{t('backups.appsPaused')}</Banner>
          <div>
            <Button variant="primary" icon={Download} busy={preparing} onClick={create}>
              {preparing ? t('backups.preparing') : t('backups.download')}
            </Button>
          </div>
        </>
      )}
    </Section>
  );
}

function RestoreBackup() {
  const input = useRef<HTMLInputElement>(null);
  const [progress, setProgress] = useState<number | null>(null);
  const [info, setInfo] = useState<{ id: string; name: string; info: BackupInfo } | null>(null);
  const [over, setOver] = useState(false);
  const [busy, setBusy] = useState(false);
  const pick = async (file: File | undefined) => {
    if (!file) return;
    setProgress(0);
    try {
      const id = await uploadFile(file, (p) => setProgress(p.total ? (p.sent / p.total) * 100 : 100));
      const r = await api.post('/backups/restore', { body: { upload_id: id, dry_run: true } });
      if (r && 'apps' in r) setInfo({ id, name: file.name, info: r });
    } catch (e) {
      toast({ text: t('backups.uploadFailed', { reason: errorText(e) }), tone: 'danger' });
    }
    setProgress(null);
  };
  const restore = async () => {
    if (!info) return;
    setBusy(true);
    const op = await startOperation(() => api.post('/backups/restore', { body: { upload_id: info.id, dry_run: false } }));
    setBusy(false);
    if (op) setInfo(null);
  };
  return (
    <Section id="restore" title={t('backups.restore')} icon={History} description={t('backups.restoreDesc')}>
      <div
        class={`${c.dropzone} ${over ? c.dropzoneActive : ''}`}
        onDragOver={(e) => {
          e.preventDefault();
          setOver(true);
        }}
        onDragLeave={() => setOver(false)}
        onDrop={(e) => {
          e.preventDefault();
          setOver(false);
          void pick(e.dataTransfer?.files[0]);
        }}
      >
        <FileArchive size={32} aria-hidden="true" />
        <p>{t('backups.drop')}</p>
        <input ref={input} type="file" accept=".xz,.tar.xz,application/x-xz" hidden onChange={(e) => void pick((e.currentTarget as HTMLInputElement).files?.[0])} />
        <Button icon={Upload} onClick={() => input.current?.click()} disabled={progress !== null}>
          {t('backups.choose')}
        </Button>
      </div>
      {progress !== null && (
        <div role="status">
          <p class={c.between}>
            <span>{t('backups.uploading')}</span>
            <span class="num">{percent(progress)}</span>
          </p>
          <ProgressBar value={progress} label={t('backups.uploading')} />
        </div>
      )}
      <Dialog
        open={!!info}
        onClose={() => setInfo(null)}
        title={t('backups.confirmTitle')}
        footer={
          <>
            <Button onClick={() => setInfo(null)}>{t('common.cancel')}</Button>
            <Button variant="danger" busy={busy} onClick={() => void restore()}>
              {t('backups.restoreNow')}
            </Button>
          </>
        }
      >
        {info && (
          <>
            <Facts
              items={[
                [t('backups.file'), info.name],
                [t('backups.created'), info.info.created ? dateTime(info.info.created) : null],
                [t('backups.apps'), info.info.apps.length ? list(info.info.apps.map(appTitle)) : t('backups.noApps')],
                [t('backups.folders'), advanced.value && info.info.data_dirs.length > 0 && <span class="mono">{info.info.data_dirs.join(', ')}</span>],
              ]}
            />
            {info.info.same_device === false && <Banner tone="warn">{t('backups.otherDevice')}</Banner>}
            <Banner tone="warn">{t('backups.restoreWarn')}</Banner>
          </>
        )}
      </Dialog>
    </Section>
  );
}

function Factory() {
  const { data, error, reload, sig } = useGet('/backups/factory');
  const restore = async () => {
    if (!(await confirm({ title: t('backups.factoryRestoreTitle'), body: t('backups.factoryRestoreBody'), confirm: t('backups.restoreNow'), tone: 'danger' }))) return;
    await startOperation(() => api.post('/backups/factory/restore'));
  };
  const remove = async () => {
    if (!(await confirm({ title: t('backups.factoryDeleteTitle'), body: t('backups.factoryDeleteBody'), confirm: t('backups.delete'), tone: 'danger' }))) return;
    try {
      await api.del('/backups/factory');
      sig.value = { available: false } as FactoryBackup;
      toast({ text: t('backups.factoryDeleted'), tone: 'ok' });
    } catch (e) {
      toast({ text: errorText(e), tone: 'danger' });
    }
  };
  if (error && !data) return <ErrorState error={error} retry={reload} />;
  if (!data?.available) return null;
  return (
    <Section id="factory" title={t('backups.factory')} description={t('backups.factoryDesc')}>
      <Facts
        items={[
          [t('backups.created'), data.created ? dateTime(data.created) : null],
          [t('backups.size'), data.size ? bytes(data.size) : null],
        ]}
      />
      <div class={s.powerRow}>
        <Button variant="primary" icon={History} onClick={() => void restore()}>
          {t('backups.restoreNow')}
        </Button>
        <LinkButton href={apiUrl('/backups/factory/archive')} download="factory-backup.tar.xz" icon={Download}>
          {t('backups.downloadFactory')}
        </LinkButton>
        <Button variant="danger" icon={Trash} onClick={() => void remove()}>
          {t('backups.delete')}
        </Button>
      </div>
    </Section>
  );
}

export default function Backups() {
  return (
    <Page title={t('nav.backups')} back={{ href: '/settings', label: t('nav.settings') }} subtitle={t('nav.backupsDesc')} narrow>
      <Factory />
      <CreateBackup />
      <RestoreBackup />
    </Page>
  );
}
