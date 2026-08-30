import { Buffer } from 'buffer';

declare global {
  interface Window {
    Buffer: typeof Buffer;
  }
}

// Solana web3.js expects a global Buffer; must be imported before it.
window.Buffer = Buffer;
