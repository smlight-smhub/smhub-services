// Sign-in and accounts: require a password or keep the hub open, the
// accounts list, adding one, changing a password, signing out.
import { useState } from 'preact/hooks';
import { KeyRound, Lock, LockOpen, LogOut, Pencil, Plus, Trash, UserPlus, Users } from 'lucide-preact';
import { api } from '../../api/client';
import type { Security, Token, User } from '../../api/types';
import { t } from '../../text/i18n';
import { relative } from '../../text/format';
import { CopyButton } from '../../kit/Bits';
import { confirm } from '../../state/confirm';
import { useGet } from '../../state/get';
import { errorText, save, useFieldErrors } from '../../state/save';
import { USERNAME_EXAMPLE, session, sessionChanged, signOut } from '../../state/session';
import { toast } from '../../state/toasts';
import { Button, IconButton } from '../../kit/Button';
import { Dialog } from '../../kit/Dialog';
import { Banner, ErrorState, Loading } from '../../kit/Feedback';
import { Switch, TextField } from '../../kit/Field';
import { Actions, Page, Section } from '../../kit/Page';
import c from '../common.module.css';

function passwordProblem(pw: string, again: string): Record<string, string> {
  const e: Record<string, string> = {};
  if (pw.length < 8) e.password = t('accounts.tooShort');
  else if (pw.length > 128) e.password = t('accounts.tooLong');
  if (again !== pw) e.again = t('accounts.mismatch');
  return e;
}

function EnableForm({ onDone }: { onDone: () => void }) {
  // empty, with an example: the factory account's name is not offered as the owner's
  const [username, setUsername] = useState('');
  const [pw, setPw] = useState('');
  const [again, setAgain] = useState('');
  const [tried, setTried] = useState(false);
  const [busy, setBusy] = useState(false);
  const errors = useFieldErrors();
  const local: Record<string, string> = tried ? { ...passwordProblem(pw, again), ...(username.trim() ? {} : { username: t('form.required') }) } : {};
  const submit = async (e: Event) => {
    e.preventDefault();
    setTried(true);
    if (Object.keys({ ...passwordProblem(pw, again), ...(username.trim() ? {} : { u: 1 }) }).length) return;
    setBusy(true);
    const s = await save({ send: () => api.post('/security/enable', { body: { username: username.trim(), password: pw } }), errors, success: t('accounts.enabled') });
    setBusy(false);
    if (s) {
      sessionChanged(s);
      onDone();
    }
  };
  return (
    <form class={c.form} onSubmit={submit} noValidate>
      <TextField label={t('accounts.username')} value={username} onValue={setUsername} placeholder={USERNAME_EXAMPLE} autoComplete="username" autoCapitalize="off" required error={local.username ?? errors.value.username} />
      <TextField label={t('accounts.password')} type="password" value={pw} onValue={setPw} autoComplete="new-password" required hint={t('accounts.passwordHint')} error={local.password ?? errors.value.password} />
      <TextField label={t('accounts.again')} type="password" value={again} onValue={setAgain} autoComplete="new-password" required error={local.again} />
      <Actions>
        <Button type="submit" variant="primary" icon={Lock} busy={busy}>
          {t('accounts.enable')}
        </Button>
      </Actions>
    </form>
  );
}

function PasswordDialog({ user, onClose }: { user: User | 'new' | null; onClose: () => void }) {
  const isNew = user === 'new';
  const self = !isNew && user && user.id === session.value?.user.id;
  const [username, setUsername] = useState('');
  const [current, setCurrent] = useState('');
  const [pw, setPw] = useState('');
  const [again, setAgain] = useState('');
  const [tried, setTried] = useState(false);
  const [busy, setBusy] = useState(false);
  const errors = useFieldErrors();
  const local = tried ? passwordProblem(pw, again) : {};
  const submit = async () => {
    setTried(true);
    if (Object.keys(passwordProblem(pw, again)).length || (isNew && !username.trim())) return;
    setBusy(true);
    const ok = isNew
      ? await save({ send: () => api.post('/users', { body: { username: username.trim(), password: pw } }), errors, success: t('accounts.added', { name: username.trim() }) })
      : await save({
          send: () => api.patch('/users/{id}', { path: { id: (user as User).id }, body: self ? { password: pw, current_password: current } : { password: pw } }),
          errors,
          success: t('accounts.changed'),
        });
    setBusy(false);
    if (ok) onClose();
  };
  return (
    <Dialog
      open={!!user}
      onClose={onClose}
      onSubmit={submit}
      title={isNew ? t('accounts.addTitle') : t('accounts.changeTitle', { name: (user as User | null)?.username ?? '' })}
      size="sm"
      footer={
        <>
          <Button onClick={onClose}>{t('common.cancel')}</Button>
          <Button type="submit" variant="primary" busy={busy}>
            {isNew ? t('accounts.add') : t('accounts.change')}
          </Button>
        </>
      }
    >
      {isNew && <TextField label={t('accounts.username')} value={username} onValue={setUsername} autoComplete="off" autoCapitalize="off" required error={errors.value.username} autoFocus />}
      {self && <TextField label={t('accounts.current')} type="password" value={current} onValue={setCurrent} autoComplete="current-password" required error={errors.value.current_password} autoFocus />}
      <TextField label={t('accounts.newPassword')} type="password" value={pw} onValue={setPw} autoComplete="new-password" required hint={t('accounts.passwordHint')} error={local.password ?? errors.value.password} autoFocus={!isNew && !self} />
      <TextField label={t('accounts.again')} type="password" value={again} onValue={setAgain} autoComplete="new-password" required error={local.again} />
    </Dialog>
  );
}

/** API tokens for Home Assistant and scripts: made here, shown once, revoked here. */
function Tokens() {
  const list = useGet('/tokens');
  const [open, setOpen] = useState(false);
  const [name, setName] = useState('');
  const [made, setMade] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const errors = useFieldErrors();
  const create = async () => {
    if (!name.trim()) return;
    setBusy(true);
    const r = await save({ send: () => api.post('/tokens', { body: { name: name.trim() } }), errors });
    setBusy(false);
    if (r) {
      setMade(r.token);
      void list.reload();
    }
  };
  const revoke = async (tk: Token) => {
    if (!(await confirm({ title: t('tokens.revokeTitle', { name: tk.name }), body: t('tokens.revokeBody'), confirm: t('tokens.revoke'), tone: 'danger' }))) return;
    try {
      await api.del('/tokens/{id}', { path: { id: tk.id } });
      list.sig.value = list.sig.peek()?.filter((x) => x.id !== tk.id);
      toast({ text: t('tokens.revoked', { name: tk.name }), tone: 'ok' });
    } catch (e) {
      toast({ text: errorText(e), tone: 'danger' });
    }
  };
  const close = () => {
    setOpen(false);
    setMade(null);
    setName('');
  };
  return (
    <Section
      id="tokens"
      title={t('tokens.title')}
      icon={KeyRound}
      description={t('tokens.desc')}
      actions={
        <Button size="sm" icon={Plus} onClick={() => setOpen(true)}>
          {t('tokens.new')}
        </Button>
      }
    >
      {list.error && !list.data ? (
        <ErrorState error={list.error} retry={list.reload} />
      ) : !list.data ? (
        <Loading />
      ) : list.data.length === 0 ? (
        <p class="muted">{t('tokens.none')}</p>
      ) : (
        <ul class={c.list}>
          {list.data.map((tk) => (
            <li key={tk.id} class={c.item}>
              <KeyRound size={18} class={c.itemIcon} aria-hidden="true" />
              <div class={c.itemMain}>
                <span class={c.itemTitle}>{tk.name}</span>
                <span class={c.itemMeta}>{tk.last_used ? t('tokens.used', { when: relative(tk.last_used) }) : t('tokens.unused')}</span>
              </div>
              <span class={c.itemActions}>
                <IconButton icon={Trash} label={t('tokens.revokeNamed', { name: tk.name })} onClick={() => void revoke(tk)} />
              </span>
            </li>
          ))}
        </ul>
      )}
      <Dialog
        open={open}
        onClose={close}
        onSubmit={made ? close : create}
        title={t('tokens.new')}
        size="sm"
        footer={
          made ? (
            <Button type="submit" variant="primary">
              {t('common.done')}
            </Button>
          ) : (
            <>
              <Button onClick={close}>{t('common.cancel')}</Button>
              <Button type="submit" variant="primary" busy={busy} disabled={!name.trim()}>
                {t('tokens.make')}
              </Button>
            </>
          )
        }
      >
        {made ? (
          <>
            <Banner tone="warn">{t('tokens.once')}</Banner>
            <div class={c.keyBox}>
              <span class={`${c.keyText} mono`}>{made}</span>
              <CopyButton text={made} label={t('common.copy')} />
            </div>
            <p class="muted">{t('tokens.howTo')}</p>
          </>
        ) : (
          <TextField label={t('tokens.name')} hint={t('tokens.nameHint')} value={name} onValue={setName} maxLength={40} required error={errors.value.name} autoFocus />
        )}
      </Dialog>
    </Section>
  );
}

export default function Accounts() {
  const sec = useGet('/security');
  const users = useGet('/users', { skip: !sec.data?.required });
  const [editing, setEditing] = useState<User | 'new' | null>(null);
  const me = session.value?.user;

  const disable = async () => {
    const ok = await confirm({ title: t('accounts.disableTitle'), body: t('accounts.disableBody'), confirm: t('accounts.disable'), tone: 'danger' });
    if (!ok) return;
    const r = await save<Security>({ target: sec.sig, send: () => api.post('/security/disable'), success: t('accounts.disabled') });
    if (r) {
      const s = session.peek();
      if (s) sessionChanged({ ...s, mode: 'open' });
    }
  };
  const remove = async (u: User) => {
    if (!(await confirm({ title: t('accounts.removeTitle', { name: u.username }), body: t('accounts.removeBody'), confirm: t('accounts.remove'), tone: 'danger' }))) return;
    try {
      await api.del('/users/{id}', { path: { id: u.id } });
      users.sig.value = users.sig.peek()?.filter((x) => x.id !== u.id);
      toast({ text: t('accounts.removed', { name: u.username }), tone: 'ok' });
    } catch (e) {
      toast({ text: errorText(e), tone: 'danger' });
    }
  };

  const s = sec.data;
  return (
    <Page title={t('nav.accounts')} back={{ href: '/settings', label: t('nav.settings') }} subtitle={t('nav.accountsDesc')} narrow>
      {sec.error && !s ? (
        <ErrorState error={sec.error} retry={sec.reload} />
      ) : !s ? (
        <Loading lines={5} />
      ) : (
        <>
          <Section id="signin" title={t('accounts.signIn')} icon={s.required ? Lock : LockOpen}>
            {s.required ? (
              <>
                <Switch label={t('accounts.required')} description={t('accounts.requiredDesc', { name: s.username ?? '' })} checked onChange={() => void disable()} />
                {session.value?.mode === 'password' && (
                  <Actions>
                    <Button icon={LogOut} onClick={() => void signOut()}>
                      {t('account.signOut')}
                    </Button>
                  </Actions>
                )}
              </>
            ) : (
              <>
                <Banner tone="warn" icon={LockOpen} title={t('openHub.title')}>
                  {t('accounts.openText')}
                </Banner>
                <EnableForm onDone={() => void sec.reload()} />
              </>
            )}
          </Section>
          {s.required && (
            <Section
              id="users"
              title={t('accounts.users')}
              icon={Users}
              actions={
                <Button size="sm" icon={UserPlus} onClick={() => setEditing('new')}>
                  {t('accounts.add')}
                </Button>
              }
            >
              {users.error && !users.data ? (
                <ErrorState error={users.error} retry={users.reload} />
              ) : !users.data ? (
                <Loading />
              ) : (
                <ul class={c.list}>
                  {users.data.map((u) => (
                    <li key={u.id} class={c.item}>
                      <KeyRound size={18} class={c.itemIcon} aria-hidden="true" />
                      <div class={c.itemMain}>
                        <span class={c.itemTitle}>
                          {u.username}
                          {u.id === me?.id && <span class={`${c.tag} ${c.tagAccent}`}>{t('accounts.you')}</span>}
                        </span>
                      </div>
                      <span class={c.itemActions}>
                        <IconButton icon={Pencil} label={t('accounts.changeNamed', { name: u.username })} onClick={() => setEditing(u)} />
                        {u.id !== me?.id && users.data!.length > 1 && <IconButton icon={Trash} label={t('accounts.removeNamed', { name: u.username })} onClick={() => void remove(u)} />}
                      </span>
                    </li>
                  ))}
                </ul>
              )}
            </Section>
          )}
          <Tokens />
        </>
      )}
      <PasswordDialog
        user={editing}
        onClose={() => {
          if (editing === 'new') void users.reload();
          setEditing(null);
        }}
      />
    </Page>
  );
}
