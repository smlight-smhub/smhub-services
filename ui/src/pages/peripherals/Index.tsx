// Peripherals: the hub's own hardware (buzzer, lights, infrared, buttons, USB), each
// page as a tile with what it is for. These are parts of the device, not settings.
import { SectionIndex } from '../SectionIndex';

export default function Peripherals() {
  return <SectionIndex parent="peripherals" title="nav.peripherals" subtitle="nav.peripheralsDesc" />;
}
