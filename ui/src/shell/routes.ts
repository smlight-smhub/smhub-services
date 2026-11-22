// Every page: its address, its chunk, where it sits in the navigation, and
// when it is shown (UI level, the hub's features). The sidebar, the phone
// navigation, the router and the command palette all read this one table.
import type { LucideIcon } from 'lucide-preact';
import {
  Archive,
  BatteryCharging,
  BellRing,
  Boxes,
  Cable,
  CircleDot,
  CircuitBoard,
  Cpu,
  Folder,
  Gauge,
  Globe,
  House,
  KeyRound,
  Languages,
  Lightbulb,
  Network,
  Palette,
  Power,
  Radio,
  RefreshCw,
  ScrollText,
  Server,
  Settings,
  Shield,
  Smartphone,
  SquareTerminal,
  Tv,
  Unplug,
  Usb,
  Wifi,
  Workflow,
} from 'lucide-preact';
import type { System } from '../api/types';
import type { Key } from '../text/i18n';
import { lazy, type LazyComponent } from './lazy';

export type Group = 'main' | 'network' | 'peripherals' | 'settings' | 'tools';

export interface PageDef {
  id: string;
  path: string;
  /** Address of the nav link (defaults to path). */
  href?: string;
  label: Key;
  description?: Key;
  icon: LucideIcon;
  group: Group;
  /** Child of another page in the navigation. */
  parent?: string;
  /** Shown only at the advanced level (D35). */
  advanced?: boolean;
  /** Shown only on hubs that have the feature (D10). */
  available?: (s: System) => boolean;
  /** On the phone's bottom bar. */
  bottom?: boolean;
  /** Extra words the command palette matches. */
  keywords?: string;
  page: LazyComponent;
}

const page = (load: () => Promise<{ default: unknown }>) => lazy(load as Parameters<typeof lazy>[0]);

export const PAGES: PageDef[] = [
  {
    id: 'apps',
    path: '/apps',
    label: 'nav.apps',
    description: 'nav.appsDesc',
    icon: Boxes,
    group: 'main',
    bottom: true,
    keywords: 'store install packages',
    page: page(() => import('../pages/apps/Store')),
  },
  {
    id: 'network',
    path: '/network',
    label: 'nav.network',
    description: 'nav.networkDesc',
    icon: Network,
    group: 'main',
    bottom: true,
    keywords: 'ip address internet',
    page: page(() => import('../pages/network/Overview')),
  },
  {
    id: 'ethernet',
    path: '/network/ethernet',
    label: 'nav.ethernet',
    description: 'nav.ethernetDesc',
    icon: Cable,
    group: 'network',
    parent: 'network',
    keywords: 'lan dhcp static ip dns gateway mtu',
    page: page(() => import('../pages/network/Ethernet')),
  },
  {
    id: 'wifi',
    path: '/network/wifi',
    label: 'nav.wifi',
    description: 'nav.wifiDesc',
    icon: Wifi,
    group: 'network',
    parent: 'network',
    available: (s) => s.features.wifi,
    keywords: 'wireless wlan ssid',
    page: page(() => import('../pages/network/Wifi')),
  },
  {
    id: 'vpn',
    path: '/network/vpn',
    label: 'nav.vpn',
    description: 'nav.vpnDesc',
    icon: Shield,
    group: 'network',
    parent: 'network',
    keywords: 'wireguard tunnel remote',
    page: page(() => import('../pages/network/Vpn')),
  },
  {
    id: 'lte',
    path: '/network/lte',
    label: 'nav.lte',
    description: 'nav.lteDesc',
    icon: Smartphone,
    group: 'network',
    parent: 'network',
    available: (s) => s.features.lte,
    keywords: '4g modem mobile sim apn',
    page: page(() => import('../pages/network/Lte')),
  },
  {
    id: 'radios',
    path: '/radios',
    label: 'nav.radios',
    description: 'nav.radiosDesc',
    icon: Radio,
    group: 'main',
    available: (s) => s.radios.length > 0,
    keywords: 'zigbee thread z-wave firmware flash coordinator router',
    page: page(() => import('../pages/radios/Radios')),
  },
  // the hub's own hardware: parts of the device, not settings
  {
    id: 'peripherals',
    path: '/peripherals',
    label: 'nav.peripherals',
    description: 'nav.peripheralsDesc',
    icon: CircuitBoard,
    group: 'main',
    keywords: 'hardware device',
    page: page(() => import('../pages/peripherals/Index')),
  },
  {
    id: 'buzzer',
    path: '/peripherals/buzzer',
    label: 'nav.buzzer',
    description: 'nav.buzzerDesc',
    icon: BellRing,
    group: 'peripherals',
    parent: 'peripherals',
    available: (s) => s.features.buzzer,
    keywords: 'buzzer sound melody rtttl beep event sounds quiet hours mute audio',
    page: page(() => import('../pages/peripherals/Buzzer')),
  },
  {
    id: 'lights',
    path: '/peripherals/lights',
    label: 'nav.lights',
    description: 'nav.lightsDesc',
    icon: Lightbulb,
    group: 'peripherals',
    parent: 'peripherals',
    keywords: 'ambient light ring ambilight led leds night mode all off colour color effect brightness',
    page: page(() => import('../pages/peripherals/Lights')),
  },
  {
    id: 'infrared',
    path: '/peripherals/infrared',
    label: 'nav.infrared',
    description: 'nav.infraredDesc',
    icon: Tv,
    group: 'peripherals',
    parent: 'peripherals',
    available: (s) => s.features.ir,
    keywords: 'ir remote control learn tv air conditioner blaster',
    page: page(() => import('../pages/peripherals/Infrared')),
  },
  {
    id: 'buttons',
    path: '/peripherals/buttons',
    label: 'nav.buttons',
    description: 'nav.buttonsDesc',
    icon: CircleDot,
    group: 'peripherals',
    parent: 'peripherals',
    keywords: 'buttons btn1 btn2 rst press double long gesture action',
    page: page(() => import('../pages/peripherals/Buttons')),
  },
  // UPS board: in development builds only, until the page is released (the API is there
  // either way); VITE_UPS=1 puts it in a release build. Left out, its code is not in the
  // package at all (docs/peripherals.md, "UPS board").
  ...(import.meta.env.DEV || import.meta.env.VITE_UPS === '1'
    ? [
        {
          id: 'ups',
          path: '/peripherals/ups',
          label: 'nav.ups',
          description: 'nav.upsDesc',
          icon: BatteryCharging,
          group: 'peripherals',
          parent: 'peripherals',
          available: (s: System) => s.family === 'smhub',
          keywords: 'ups battery backup power j10 i2c',
          page: page(() => import('../pages/peripherals/Ups')),
        } satisfies PageDef,
      ]
    : []),
  {
    id: 'usb',
    path: '/peripherals/usb',
    label: 'nav.usb',
    description: 'nav.usbDesc',
    icon: Usb,
    group: 'peripherals',
    parent: 'peripherals',
    advanced: true,
    keywords: 'host device otg',
    page: page(() => import('../pages/peripherals/Usb')),
  },
  {
    id: 'updates',
    path: '/updates',
    label: 'nav.updates',
    description: 'nav.updatesDesc',
    icon: RefreshCw,
    group: 'main',
    keywords: 'firmware os upgrade channel',
    page: page(() => import('../pages/updates/Updates')),
  },
  {
    id: 'settings',
    path: '/settings',
    label: 'nav.settings',
    description: 'nav.settingsDesc',
    icon: Settings,
    group: 'main',
    bottom: true,
    page: page(() => import('../pages/settings/Index')),
  },
  {
    id: 'general',
    path: '/settings/general',
    label: 'nav.general',
    description: 'nav.generalDesc',
    icon: Settings,
    group: 'settings',
    parent: 'settings',
    keywords: 'hostname name time zone clock sd card',
    page: page(() => import('../pages/settings/General')),
  },
  {
    id: 'accounts',
    path: '/settings/accounts',
    label: 'nav.accounts',
    description: 'nav.accountsDesc',
    icon: KeyRound,
    group: 'settings',
    parent: 'settings',
    keywords: 'password login users security sign in',
    page: page(() => import('../pages/settings/Accounts')),
  },
  {
    id: 'appearance',
    path: '/settings/appearance',
    label: 'nav.appearance',
    description: 'nav.appearanceDesc',
    icon: Palette,
    group: 'settings',
    parent: 'settings',
    keywords: 'theme dark light colour color tone text size spacing density details simple advanced home layout',
    page: page(() => import('../pages/settings/Appearance')),
  },
  {
    id: 'regional',
    path: '/settings/regional',
    label: 'nav.regional',
    description: 'nav.regionalDesc',
    icon: Languages,
    group: 'settings',
    parent: 'settings',
    keywords: 'language region formats units metric imperial temperature currency clock time zone',
    page: page(() => import('../pages/settings/Regional')),
  },
  {
    id: 'mqtt',
    path: '/settings/mqtt',
    label: 'nav.mqtt',
    description: 'nav.mqttDesc',
    icon: Workflow,
    group: 'settings',
    parent: 'settings',
    keywords: 'mosquitto broker bridge users',
    page: page(() => import('../pages/settings/Mqtt')),
  },
  {
    id: 'esphome',
    path: '/settings/esphome',
    label: 'nav.esphome',
    description: 'nav.esphomeDesc',
    icon: Cpu,
    group: 'settings',
    parent: 'settings',
    available: (s) => s.features.esphome,
    keywords: 'home assistant encryption key bluetooth proxy light buzzer i2c rtos',
    page: page(() => import('../pages/settings/Esphome')),
  },
  {
    id: 'backups',
    path: '/settings/backups',
    label: 'nav.backups',
    description: 'nav.backupsDesc',
    icon: Archive,
    group: 'settings',
    parent: 'settings',
    keywords: 'backup restore factory',
    page: page(() => import('../pages/settings/Backups')),
  },
  {
    id: 'ssh',
    path: '/settings/ssh',
    label: 'nav.ssh',
    description: 'nav.sshDesc',
    icon: KeyRound,
    group: 'settings',
    parent: 'settings',
    advanced: true,
    keywords: 'secure shell keys root',
    page: page(() => import('../pages/settings/Ssh')),
  },
  {
    id: 'serial',
    path: '/settings/serial-bridge',
    label: 'nav.serial',
    description: 'nav.serialDesc',
    icon: Unplug,
    group: 'settings',
    parent: 'settings',
    advanced: true,
    keywords: 'ser2net tcp serial port baud',
    page: page(() => import('../pages/settings/SerialBridge')),
  },
  {
    id: 'system',
    path: '/settings/system',
    label: 'nav.system',
    description: 'nav.systemDesc',
    icon: Power,
    group: 'settings',
    parent: 'settings',
    keywords: 'reboot restart power off shutdown model version',
    page: page(() => import('../pages/settings/System')),
  },
  {
    id: 'terminal',
    path: '/tools/terminal',
    label: 'nav.terminal',
    description: 'nav.terminalDesc',
    icon: SquareTerminal,
    group: 'tools',
    advanced: true,
    keywords: 'console shell command line',
    page: page(() => import('../pages/tools/Terminal')),
  },
  {
    id: 'files',
    path: '/tools/files',
    label: 'nav.files',
    description: 'nav.filesDesc',
    icon: Folder,
    group: 'tools',
    advanced: true,
    keywords: 'file manager upload download folder edit',
    page: page(() => import('../pages/tools/Files')),
  },
  {
    id: 'logs',
    path: '/tools/logs',
    label: 'nav.logs',
    description: 'nav.logsDesc',
    icon: ScrollText,
    group: 'tools',
    advanced: true,
    keywords: 'journal messages syslog',
    page: page(() => import('../pages/tools/Logs')),
  },
  {
    id: 'services',
    path: '/tools/services',
    label: 'nav.services',
    description: 'nav.servicesDesc',
    icon: Server,
    group: 'tools',
    advanced: true,
    keywords: 'openrc daemons processes start stop',
    page: page(() => import('../pages/tools/Services')),
  },
];

/** Pages without a place in the navigation. */
export const AppPage = page(() => import('../pages/apps/AppPage'));
export const AppFrame = page(() => import('../pages/apps/AppFrame'));
export const Setup = page(() => import('./Setup'));

export const HOME = { id: 'home', path: '/', label: 'nav.home' as Key, icon: House };
export const ICON_GAUGE = Gauge;
export const ICON_GLOBE = Globe;

/** Whether a page is shown to this user on this hub. */
export function visible(p: PageDef, sys: System | null | undefined, advanced: boolean): boolean {
  if (p.advanced && !advanced) return false;
  if (p.available && sys && !p.available(sys)) return false;
  return true;
}

export function pageById(id: string): PageDef | undefined {
  return PAGES.find((p) => p.id === id);
}

/** The page whose address the location starts with (the longest match). */
export function activePage(location: string): PageDef | undefined {
  let best: PageDef | undefined;
  for (const p of PAGES) {
    if (location === p.path || location.startsWith(p.path + '/')) {
      if (!best || p.path.length > best.path.length) best = p;
    }
  }
  return best;
}
