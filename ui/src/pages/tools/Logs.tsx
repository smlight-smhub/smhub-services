// Logs: every log the hub keeps; the chosen one is tailed, filtered,
// followed live and downloadable. The log is part of the address (?name=…).
import { useLocation, useSearch } from 'wouter-preact';
import { ScrollText } from 'lucide-preact';
import { t } from '../../text/i18n';
import { bytes, compare } from '../../text/format';
import { useGet } from '../../state/get';
import { appTitle } from '../../kit/Bits';
import { EmptyState, ErrorState, Loading } from '../../kit/Feedback';
import { LogView } from '../../kit/LogView';
import { Page, Section } from '../../kit/Page';
import { cx } from '../../kit/cx';
import s from './Tools.module.css';

export default function Logs() {
  const { data, error, reload } = useGet('/logs');
  const search = useSearch();
  const [, navigate] = useLocation();
  const logs = [...(data ?? [])].sort((a, b) => compare(a.name, b.name));
  const chosen = new URLSearchParams(search).get('name') ?? logs[0]?.name;
  return (
    <Page title={t('nav.logs')} subtitle={t('nav.logsDesc')}>
      {error && !data ? (
        <ErrorState error={error} retry={reload} />
      ) : !data ? (
        <Loading lines={8} />
      ) : !logs.length ? (
        <EmptyState icon={ScrollText} title={t('logs.none')} />
      ) : (
        <div class={s.logs}>
          <Section title={t('logs.all')}>
            <nav aria-label={t('logs.all')}>
              <ul class={s.logList}>
                {logs.map((l) => (
                  <li key={l.name}>
                    <button
                      type="button"
                      class={cx(s.logItem, l.name === chosen && s.logItemOn)}
                      aria-current={l.name === chosen ? 'true' : undefined}
                      onClick={() => navigate(`/tools/logs?name=${encodeURIComponent(l.name)}`, { replace: true })}
                    >
                      <strong>{l.app ? appTitle(l.app) : l.name}</strong>
                      <span class="muted">
                        {l.app ? `${l.name} · ` : ''}
                        {bytes(l.size)}
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
            </nav>
          </Section>
          {chosen && (
            <Section title={chosen}>
              <LogView key={chosen} name={chosen} />
            </Section>
          )}
        </div>
      )}
    </Page>
  );
}
