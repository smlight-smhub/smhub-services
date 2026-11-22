// A section's overview: each of its pages as a tile with what it is for (Settings, Peripherals).
import { Link } from 'wouter-preact';
import { ChevronRight, Sparkles } from 'lucide-preact';
import { t, type Key } from '../text/i18n';
import { advanced } from '../state/prefs';
import { system } from '../state/system';
import { PAGES, visible } from '../shell/routes';
import { Page } from '../kit/Page';
import c from './common.module.css';

export function SectionIndex({ parent, title, subtitle }: { parent: string; title: Key; subtitle: Key }) {
  const sys = system.value;
  const adv = advanced.value;
  const items = PAGES.filter((p) => p.parent === parent && visible(p, sys, adv));
  const hidden = PAGES.filter((p) => p.parent === parent && p.advanced && !adv && visible(p, sys, true)).length;
  return (
    <Page title={t(title)} subtitle={t(subtitle)}>
      <ul class={c.grid}>
        {items.map((p) => {
          const I = p.icon;
          return (
            <li key={p.id}>
              <Link href={p.path} class={c.linkCard} onPointerEnter={p.page.preload}>
                <span class={c.linkIcon} aria-hidden="true">
                  <I size={22} />
                </span>
                <span class={c.linkText}>
                  <span class={c.linkTitle}>{t(p.label)}</span>
                  {p.description && <span class={c.linkDesc}>{t(p.description)}</span>}
                </span>
                <ChevronRight size={20} class={c.chevron} aria-hidden="true" />
              </Link>
            </li>
          );
        })}
      </ul>
      {hidden > 0 && (
        <p class={`${c.note} ${c.inline}`}>
          <Sparkles size={16} aria-hidden="true" />
          {t('settings.moreInAdvanced', { count: hidden })}
        </p>
      )}
    </Page>
  );
}
