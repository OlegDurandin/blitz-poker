import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { reloadIfStale } from './freshness';
import { platform } from './platform';
import './styles.css';

platform.init();

// A stale cached build reloads once before anything renders.
void reloadIfStale().then((reloading) => {
  if (reloading) return;
  createRoot(document.getElementById('root')!).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
});
