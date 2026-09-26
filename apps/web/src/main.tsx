// MUST be the first import to ensure Buffer is available globally
import './polyfills';
import './index.css';
import { createRoot } from 'react-dom/client';
import { BrowserRouter } from 'react-router-dom';

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

const rootElement = document.getElementById('root');
if (!rootElement) throw new Error('Failed to find the root element');

createRoot(rootElement).render(
  <BrowserRouter basename={import.meta.env.BASE_URL}>
    <WalletProvider cluster={clusterOrDefault()}>
      <App />
    </WalletProvider>
  </BrowserRouter>,
);
