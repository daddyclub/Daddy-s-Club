// MUST be the first import to ensure Buffer is available globally
import './polyfills';
import './index.css';
import { createRoot } from 'react-dom/client';
import { HashRouter } from 'react-router-dom';

import App from './App';
import { WalletProvider } from './hooks/useWallet';
import { type Cluster, webEnv } from './lib/env';

/**
 * Кластер для гаманця. Хибна конфігурація тут не валить застосунок: про неї
 * скаже екран, який читає ланцюг, — там вона названа й пояснена (`EnvError`).
 */
function clusterOrDefault(): Cluster {
  try {
    return webEnv().cluster;
  } catch {
    return 'localnet';
  }
}

// A hash router: GitHub Pages knows nothing about client-side routes and serves
// any unknown path with status 404. Under `#/…` every address is the app's own
// `index.html`, served with 200, whatever prefix the site is published under.
const rootElement = document.getElementById('root');
if (!rootElement) throw new Error('Failed to find the root element');

createRoot(rootElement).render(
  <HashRouter>
    <WalletProvider cluster={clusterOrDefault()}>
      <App />
    </WalletProvider>
  </HashRouter>,
);
