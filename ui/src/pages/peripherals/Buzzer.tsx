// Buzzer: play a sound, the sounds for hub events, mute and quiet hours, and the user's own
// melodies (full-size SMHUB). The 3.5 mm audio output joins this page once the OS has a driver.
import { BellRing } from 'lucide-preact';
import { t } from '../../text/i18n';
import { Banner, EmptyState, ErrorState, Loading } from '../../kit/Feedback';
import { Page } from '../../kit/Page';
import { ActivityList, EspNotice, EventSounds, Sounds, useHardware } from './hardware-parts';

export default function Buzzer() {
  const { hw, set, settings, errors, patch } = useHardware();
  return (
    <Page title={t('nav.buzzer')} back={{ href: '/peripherals', label: t('nav.peripherals') }} subtitle={t('nav.buzzerDesc')} narrow>
      {set.error && !settings ? (
        <ErrorState error={set.error} retry={set.reload} />
      ) : !hw || !settings ? (
        <Loading lines={6} />
      ) : !hw.buzzer.available ? (
        <EmptyState icon={BellRing} title={t('hw.nothing')} />
      ) : (
        <>
          <EspNotice hw={hw} owners={[hw.buzzer.owner]} />
          {Object.values(errors.value)[0] && <Banner tone="danger">{Object.values(errors.value)[0]}</Banner>}
          <Sounds hw={hw} settings={settings} patch={patch} />
          <EventSounds settings={settings} patch={patch} />
          <ActivityList hw={hw} kinds={['sound', 'find']} />
        </>
      )}
    </Page>
  );
}
