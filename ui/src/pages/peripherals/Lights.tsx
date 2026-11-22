// Lights: the ambient light ring (full-size SMHUB) with hub events shown on it, and the service
// LEDs with "all off" and night mode (every model).
import { Lightbulb } from 'lucide-preact';
import { t } from '../../text/i18n';
import { Banner, EmptyState, ErrorState, Loading } from '../../kit/Feedback';
import { Page } from '../../kit/Page';
import { ActivityList, EspNotice, Leds, LightCard, useHardware } from './hardware-parts';

export default function Lights() {
  const { hw, set, settings, errors, patch } = useHardware();
  return (
    <Page title={t('nav.lights')} back={{ href: '/peripherals', label: t('nav.peripherals') }} subtitle={t('nav.lightsDesc')} narrow>
      {set.error && !settings ? (
        <ErrorState error={set.error} retry={set.reload} />
      ) : !hw || !settings ? (
        <Loading lines={8} />
      ) : !hw.light.available && !hw.leds.available ? (
        <EmptyState icon={Lightbulb} title={t('hw.nothing')} />
      ) : (
        <>
          <EspNotice hw={hw} owners={[hw.light.owner, hw.leds.owner]} />
          {Object.values(errors.value)[0] && <Banner tone="danger">{Object.values(errors.value)[0]}</Banner>}
          {hw.light.available && <LightCard hw={hw} settings={settings} patch={patch} />}
          {hw.leds.available && <Leds hw={hw} settings={settings} patch={patch} />}
          <ActivityList hw={hw} kinds={['light', 'leds', 'signal', 'find']} />
        </>
      )}
    </Page>
  );
}
