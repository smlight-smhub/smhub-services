// Readable names for things that only have identifiers: package names
// ("zigbee2mqtt") and setting keys from packages ("mqtt_base_topic").

const WORDS: Record<string, string> = {
  smhub: 'SMHUB',
  mqtt: 'MQTT',
  esphome: 'ESPHome',
  ha: 'HA',
  ip: 'IP',
  id: 'ID',
  url: 'URL',
  api: 'API',
  ui: 'UI',
  usb: 'USB',
  dns: 'DNS',
  tcp: 'TCP',
  udp: 'UDP',
  ssl: 'SSL',
  tls: 'TLS',
  http: 'HTTP',
  https: 'HTTPS',
  ieee: 'IEEE',
  pan: 'PAN',
  rtos: 'RTOS',
  os: 'OS',
  abi: 'ABI',
  zha: 'ZHA',
  ble: 'BLE',
  led: 'LED',
};

function word(w: string, first: boolean): string {
  const known = WORDS[w.toLowerCase()];
  if (known) return known;
  return first ? w.charAt(0).toUpperCase() + w.slice(1) : w;
}

/** "zigbee2mqtt" → "Zigbee2mqtt", "smhub-broker" → "SMHUB Broker". */
export function appTitle(name: string): string {
  return name
    .split(/[-_]+/)
    .filter(Boolean)
    .map((w) => word(w, true))
    .join(' ');
}

/** A setting key from a package as words: "mqtt_base_topic" → "MQTT base topic". */
export function keyTitle(key: string): string {
  const last = key.split('.').pop() ?? key;
  const words = last
    .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
    .split(/[\s_-]+/)
    .filter(Boolean);
  return words.map((w, i) => word(i === 0 ? w : w.toLowerCase(), i === 0)).join(' ') || key;
}
