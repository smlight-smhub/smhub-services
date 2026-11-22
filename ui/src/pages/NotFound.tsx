import { House, MapPinOff } from 'lucide-preact';
import { t } from '../text/i18n';
import { LinkButton } from '../kit/Button';
import { EmptyState } from '../kit/Feedback';
import { Page } from '../kit/Page';

export function NotFound() {
  return (
    <Page title={t('notFound.title')} narrow>
      <EmptyState
        icon={MapPinOff}
        title={t('notFound.heading')}
        action={
          <LinkButton href="/" variant="primary" icon={House}>
            {t('notFound.home')}
          </LinkButton>
        }
      >
        {t('notFound.text')}
      </EmptyState>
    </Page>
  );
}
