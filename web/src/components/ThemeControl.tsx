import { useSyncExternalStore } from 'react';
import { Monitor, Moon, Sun } from 'lucide-react';
import { getThemePreference, setThemePreference, subscribeTheme, type ThemePreference } from '../theme';

const order: ThemePreference[] = ['system', 'light', 'dark'];
const icons = { system: Monitor, light: Sun, dark: Moon };
const labels = { system: 'System theme', light: 'Light theme', dark: 'Dark theme' };

/** One button that cycles system → light → dark. */
export function ThemeControl() {
  const preference = useSyncExternalStore(subscribeTheme, getThemePreference, () => 'system' as ThemePreference);
  const next = order[(order.indexOf(preference) + 1) % order.length];
  const Icon = icons[preference];
  return (
    <button
      type="button"
      className="theme-trigger"
      aria-label={`${labels[preference]}; switch to ${labels[next].toLowerCase()}`}
      title={`${labels[preference]} (click for ${labels[next].toLowerCase()})`}
      onClick={() => setThemePreference(next)}
    >
      <Icon size={16} strokeWidth={1.8} aria-hidden="true" />
    </button>
  );
}
