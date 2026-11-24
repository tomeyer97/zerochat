import { createRoot } from 'react-dom/client';
import '@fontsource-variable/fraunces';
import '@fontsource-variable/spline-sans';
import '@xyflow/react/dist/style.css';
import './styles.css';
import { initializeTheme } from './theme';
import { App } from './App';
import { ErrorBoundary } from './common';
import { hashFromPath } from './route';

// A path link (`/runs/local/<id>`) becomes the hash route it names before the app reads the route.
const fromPath = hashFromPath(location.pathname, location.search, location.hash);
if (fromPath) history.replaceState(null, '', `/${fromPath}`);

initializeTheme();
createRoot(document.getElementById('root')!).render(
  <ErrorBoundary>
    <App />
  </ErrorBoundary>
);
