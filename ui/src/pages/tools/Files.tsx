// The file manager: browse the hub's roots, upload in pieces with progress,
// download, create folders, rename, move or copy, delete, and open or edit
// small text files. The folder is part of the address (?path=…).
import { useRef, useState } from 'preact/hooks';
import { useLocation, useSearch } from 'wouter-preact';
import { Download,File as FileIcon, FilePen, Folder as FolderIcon, FolderInput, FolderPlus, Link as LinkIcon, PencilLine, RefreshCw, Trash, Upload } from 'lucide-preact';
import { api, apiUrl, buildUrl, request } from '../../api/client';
import type { FolderEntry } from '../../api/types';
import { PIECE, uploadFile } from '../../api/upload';
import { t } from '../../text/i18n';
import { bytes, compare, dateTime, percent, relative } from '../../text/format';
import { confirm } from '../../state/confirm';
import { useGet } from '../../state/get';
import { errorText } from '../../state/save';
import { toast } from '../../state/toasts';
import { hubTime, useNow } from '../../state/system';
import { Button } from '../../kit/Button';
import { Dialog } from '../../kit/Dialog';
import { Banner, EmptyState, ErrorState, Loading, ProgressBar } from '../../kit/Feedback';
import { Checkbox, SelectField, TextArea, TextField } from '../../kit/Field';
import { ActionMenu } from '../../kit/Menu';
import { Page, Section } from '../../kit/Page';
import c from '../common.module.css';
import s from './Tools.module.css';

const IMAGE = /\.(png|jpe?g|gif|webp|svg|bmp|ico)$/i;
const join = (dir: string, name: string) => (dir.endsWith('/') ? dir + name : `${dir}/${name}`);
const parent = (p: string) => p.replace(/\/[^/]+\/?$/, '') || '/';

type Dlg =
  | { kind: 'folder' }
  | { kind: 'rename'; entry: FolderEntry }
  | { kind: 'move'; entry: FolderEntry }
  | { kind: 'edit'; entry: FolderEntry; text: string; readOnly: boolean }
  | { kind: 'image'; entry: FolderEntry }
  | null;

export default function Files() {
  const search = useSearch();
  const [, navigate] = useLocation();
  const path = new URLSearchParams(search).get('path') ?? undefined;
  const { data, error, reload } = useGet('/files', { query: { path } });
  const [dlg, setDlg] = useState<Dlg>(null);
  const [uploads, setUploads] = useState<{ name: string; progress: number }[]>([]);
  const [name, setName] = useState('');
  const [copy, setCopy] = useState(false);
  const [busy, setBusy] = useState(false);
  const fileInput = useRef<HTMLInputElement>(null);
  const now = useNow('minute');
  const go = (p: string) => navigate(`/tools/files?path=${encodeURIComponent(p)}`);
  /** Opens a name dialog with its field already filled: set in the same click, not in an
   * effect afterwards, which could overwrite what was typed straight away. */
  const askName = (d: Exclude<Dlg, null>) => {
    if (d.kind === 'rename') setName(d.entry.name);
    else if (d.kind === 'move') {
      setName(join(data?.path ?? '', d.entry.name));
      setCopy(false);
    } else if (d.kind === 'folder') setName('');
    setDlg(d);
  };

  const dir = data?.path ?? '';
  const root = data?.roots.filter((r) => dir === r.path || dir.startsWith(r.path.endsWith('/') ? r.path : r.path + '/')).sort((a, b) => b.path.length - a.path.length)[0];
  const crumbs: { label: string; path: string }[] = [];
  if (root) {
    crumbs.push({ label: root.name, path: root.path });
    let cur = root.path;
    for (const part of dir.slice(root.path.length).split('/').filter(Boolean)) {
      cur = join(cur, part);
      crumbs.push({ label: part, path: cur });
    }
  }
  const entries = [...(data?.entries ?? [])].sort((a, b) => Number(b.kind === 'dir') - Number(a.kind === 'dir') || compare(a.name, b.name));

  const run = async (fn: () => Promise<unknown>, done: string) => {
    setBusy(true);
    try {
      await fn();
      toast({ text: done, tone: 'ok' });
      setDlg(null);
      void reload();
    } catch (e) {
      toast({ text: errorText(e), tone: 'danger' });
    }
    setBusy(false);
  };
  const upload = async (files: FileList | null | undefined) => {
    if (!files?.length || !data) return;
    for (const f of Array.from(files)) {
      setUploads((u) => [...u, { name: f.name, progress: 0 }]);
      try {
        const id = await uploadFile(f, (p) => setUploads((u) => u.map((x) => (x.name === f.name ? { ...x, progress: p.total ? (p.sent / p.total) * 100 : 100 } : x))));
        await api.post('/files/upload', { body: { upload_id: id, path: join(dir, f.name) } });
        toast({ text: t('files.uploaded', { name: f.name }), tone: 'ok' });
      } catch (e) {
        toast({ text: t('files.uploadFailed', { name: f.name, reason: errorText(e) }), tone: 'danger' });
      }
      setUploads((u) => u.filter((x) => x.name !== f.name));
    }
    void reload();
  };
  const open = async (e: FolderEntry) => {
    const full = join(dir, e.name);
    if (e.kind === 'dir') return go(full);
    if (IMAGE.test(e.name)) return setDlg({ kind: 'image', entry: e });
    if (e.size > PIECE) return toast({ text: t('files.tooBig'), tone: 'info' });
    try {
      const res = await request('GET', buildUrl('/files/content', undefined, { path: full }), { accept: '*/*' });
      const text = await res.text();
      setDlg({ kind: 'edit', entry: e, text, readOnly: !e.writable });
    } catch (err) {
      toast({ text: errorText(err), tone: 'danger' });
    }
  };
  const remove = async (e: FolderEntry) => {
    const ok = await confirm({ title: t('files.deleteTitle', { name: e.name }), body: e.kind === 'dir' ? t('files.deleteFolderBody') : t('files.deleteBody'), confirm: t('files.delete'), tone: 'danger' });
    if (ok) await run(() => api.del('/files', { query: { path: join(dir, e.name) } }), t('files.deleted', { name: e.name }));
  };

  return (
    <Page
      title={t('nav.files')}
      subtitle={t('nav.filesDesc')}
      actions={
        data && (
          <>
            <Button icon={RefreshCw} onClick={() => void reload()}>
              {t('files.refresh')}
            </Button>
            {data.writable && (
              <>
                <Button icon={FolderPlus} onClick={() => askName({ kind: 'folder' })}>
                  {t('files.newFolder')}
                </Button>
                <input ref={fileInput} type="file" multiple hidden onChange={(e) => void upload((e.currentTarget as HTMLInputElement).files)} />
                <Button variant="primary" icon={Upload} onClick={() => fileInput.current?.click()}>
                  {t('files.upload')}
                </Button>
              </>
            )}
          </>
        )
      }
    >
      {error && !data ? (
        <ErrorState error={error} retry={reload} />
      ) : !data ? (
        <Loading lines={8} />
      ) : (
        <Section>
          <div class={c.toolbar}>
            {data.roots.length > 1 && (
              <SelectField label={t('files.root')} hideLabel value={root?.path ?? ''} onValue={go} options={data.roots.map((r) => ({ value: r.path, label: r.name }))} />
            )}
            <nav aria-label={t('files.location')} class={s.crumbs}>
              {crumbs.map((cr, i) => (
                <span key={cr.path} class={c.inline}>
                  {i > 0 && <span class={s.crumbSep} aria-hidden="true">/</span>}
                  <button type="button" class={s.crumb} onClick={() => go(cr.path)} aria-current={i === crumbs.length - 1 ? 'page' : undefined}>
                    {cr.label}
                  </button>
                </span>
              ))}
            </nav>
          </div>
          {!data.writable && <Banner tone="info">{t('files.readOnly')}</Banner>}
          {uploads.map((u) => (
            <div key={u.name} role="status">
              <p class={c.between}>
                <span>{t('files.uploading', { name: u.name })}</span>
                <span class="num">{percent(u.progress)}</span>
              </p>
              <ProgressBar value={u.progress} label={t('files.uploading', { name: u.name })} />
            </div>
          ))}
          {entries.length ? (
            <div class={c.tableWrap}>
              <table class={c.table}>
                <thead>
                  <tr>
                    <th>{t('files.name')}</th>
                    <th class={`${c.end} ${s.hideNarrow}`}>{t('files.size')}</th>
                    <th class={s.hideNarrow}>{t('files.modified')}</th>
                    <th>
                      <span class="visually-hidden">{t('files.actions')}</span>
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {root && dir !== root.path && (
                    <tr>
                      <td colSpan={4}>
                        <button type="button" class={s.fileName} onClick={() => go(parent(dir))}>
                          <FolderIcon size={18} class={s.folderIcon} aria-hidden="true" />
                          {t('files.up')}
                        </button>
                      </td>
                    </tr>
                  )}
                  {entries.map((e) => {
                    const full = join(dir, e.name);
                    const Icon = e.kind === 'dir' ? FolderIcon : e.kind === 'link' ? LinkIcon : FileIcon;
                    return (
                      <tr key={e.name}>
                        <td>
                          <button type="button" class={s.fileName} onClick={() => void open(e)}>
                            <Icon size={18} class={e.kind === 'dir' ? s.folderIcon : undefined} aria-hidden="true" />
                            {e.name}
                          </button>
                        </td>
                        <td class={`${c.end} num muted ${s.hideNarrow}`}>{e.kind === 'dir' ? '' : bytes(e.size)}</td>
                        <td class={`muted ${s.hideNarrow}`} title={dateTime(e.modified)}>
                          {relative(e.modified, hubTime(now))}
                        </td>
                        <td class={c.end}>
                          <ActionMenu
                            label={t('files.actionsFor', { name: e.name })}
                            title={e.name}
                            actions={[
                              { label: e.kind === 'dir' ? t('files.open') : e.writable ? t('files.edit') : t('files.view'), icon: FilePen, onSelect: () => void open(e) },
                              { label: t('files.download'), icon: Download, onSelect: () => window.open(apiUrl('/files/content', undefined, { path: full }), '_self') },
                              { label: t('files.rename'), icon: PencilLine, onSelect: () => askName({ kind: 'rename', entry: e }), hidden: !e.writable },
                              { label: t('files.moveCopy'), icon: FolderInput, onSelect: () => askName({ kind: 'move', entry: e }) },
                              { label: t('files.delete'), icon: Trash, danger: true, onSelect: () => void remove(e), hidden: !e.writable },
                            ]}
                          />
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          ) : (
            <EmptyState icon={FolderIcon} title={t('files.empty')} />
          )}
        </Section>
      )}

      <Dialog
        open={dlg?.kind === 'folder' || dlg?.kind === 'rename' || dlg?.kind === 'move'}
        onClose={() => setDlg(null)}
        size="sm"
        title={dlg?.kind === 'folder' ? t('files.newFolder') : dlg?.kind === 'rename' ? t('files.renameTitle', { name: dlg.entry.name }) : dlg?.kind === 'move' ? t('files.moveTitle', { name: dlg.entry.name }) : ''}
        onSubmit={() => {
          const v = name.trim();
          if (!v || !dlg) return;
          if (dlg.kind === 'folder') void run(() => api.post('/files/folder', { body: { path: join(dir, v) } }), t('files.folderCreated', { name: v }));
          else if (dlg.kind === 'rename') void run(() => api.post('/files/move', { body: { from: join(dir, dlg.entry.name), to: join(dir, v) } }), t('files.renamed'));
          else if (dlg.kind === 'move') void run(() => api.post('/files/move', { body: { from: join(dir, dlg.entry.name), to: v, copy } }), copy ? t('files.copied') : t('files.moved'));
        }}
        footer={
          <>
            <Button onClick={() => setDlg(null)}>{t('common.cancel')}</Button>
            <Button type="submit" variant="primary" busy={busy} disabled={!name.trim() || name.includes('/') && dlg?.kind !== 'move'}>
              {dlg?.kind === 'folder' ? t('files.create') : dlg?.kind === 'rename' ? t('files.rename') : copy ? t('files.copy') : t('files.move')}
            </Button>
          </>
        }
      >
        <TextField label={dlg?.kind === 'move' ? t('files.destination') : t('files.name')} value={name} onValue={setName} mono={dlg?.kind === 'move'} autoFocus spellcheck={false} />
        {dlg?.kind === 'move' && <Checkbox label={t('files.keepOriginal')} checked={copy} onChange={setCopy} hint={t('files.keepOriginalHint')} />}
      </Dialog>

      <Dialog
        open={dlg?.kind === 'edit'}
        onClose={() => setDlg(null)}
        size="full"
        title={dlg?.kind === 'edit' ? dlg.entry.name : ''}
        description={dlg?.kind === 'edit' && dlg.readOnly ? t('files.readOnlyFile') : undefined}
        footer={
          dlg?.kind === 'edit' && (
            <>
              <Button onClick={() => setDlg(null)}>{dlg.readOnly ? t('common.close') : t('common.cancel')}</Button>
              {!dlg.readOnly && (
                <Button
                  variant="primary"
                  busy={busy}
                  onClick={() =>
                    void run(
                      () => request('PUT', buildUrl('/files/content', undefined, { path: join(dir, dlg.entry.name) }), { body: dlg.text }),
                      t('files.saved', { name: dlg.entry.name }),
                    )
                  }
                >
                  {t('common.save')}
                </Button>
              )}
            </>
          )
        }
      >
        {dlg?.kind === 'edit' && (
          <TextArea label={t('files.content')} hideLabel mono rows={20} class={s.editor} value={dlg.text} onValue={(v) => setDlg({ ...dlg, text: v })} readOnly={dlg.readOnly} />
        )}
      </Dialog>

      <Dialog open={dlg?.kind === 'image'} onClose={() => setDlg(null)} size="lg" title={dlg?.kind === 'image' ? dlg.entry.name : ''}>
        {dlg?.kind === 'image' && <img class={s.preview} src={apiUrl('/files/content', undefined, { path: join(dir, dlg.entry.name) })} alt={dlg.entry.name} />}
      </Dialog>
    </Page>
  );
}
