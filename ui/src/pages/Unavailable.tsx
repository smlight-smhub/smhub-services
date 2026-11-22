// Shown instead of a page that is advanced-only, or for a feature this hub lacks.
import { CircleSlash, Sparkles } from 'lucide-preact';
import { t } from '../text/i18n';
import { savePrefs } from '../state/prefs';
import { Button, LinkButton } from '../kit/Button';
import { EmptyState } from '../kit/Feedback';
import { Page } from '../kit/Page';

export function AdvancedOnly({ title }: { title: string }) {
  return (
    <Page title={title} narrow>
      <EmptyState
        icon={Sparkles}
        title={t('unavailable.advancedTitle')}
        action={
          <Button variant="primary" onClick={() => void savePrefs({ level: 'advanced' }, { undo: true, success: t('level.nowAdvanced') })}>
            {t('unavailable.switch')}
          </Button>
        }
      >
        {t('unavailable.advancedText')}
      </EmptyState>
    </Page>
  );
}

export function NotOnThisHub({ title }: { title: string }) {
  return (
    <Page title={title} narrow>
      <EmptyState icon={CircleSlash} title={t('unavailable.featureTitle')} action={<LinkButton href="/">{t('notFound.home')}</LinkButton>}>
        {t('unavailable.featureText')}
      </EmptyState>
    </Page>
  );
}
