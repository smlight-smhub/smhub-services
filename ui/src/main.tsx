import './tokens/tokens.css';
import './tokens/tones.css';
import './tokens/base.css';
import { render } from 'preact';
import { App } from './shell/App';
import { redirectLegacyHash } from './shell/legacy';
import { boot } from './state/session';
import './state/metrics';
import './state/operations';
import './state/power';

if (import.meta.env.DEV) void import('preact/debug');

redirectLegacyHash();
void boot();
render(<App />, document.getElementById('app')!);
