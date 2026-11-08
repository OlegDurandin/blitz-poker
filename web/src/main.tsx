import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { reloadIfStale } from './freshness';
import { platform } from './platform';
import './styles.css';

platform.init();
// Keep a bottom sheet above the keyboard in mobile WebViews.
function viewportSize() {
  const v = window.visualViewport;
  document.documentElement.style.setProperty('--visible-height', `${v?.height ?? window.innerHeight}px`);
  document.documentElement.style.setProperty('--keyboard-inset', `${Math.max(0, window.innerHeight - (v?.height ?? window.innerHeight) - (v?.offsetTop ?? 0))}px`);
}
viewportSize();
window.visualViewport?.addEventListener('resize', viewportSize);
window.visualViewport?.addEventListener('scroll', viewportSize);

// A stale cached build reloads once before anything renders.
void reloadIfStale().then((reloading) => {
  if (reloading) return;
  createRoot(document.getElementById('root')!).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
});
