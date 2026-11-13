import { useLayoutEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { AppearanceSettingsPanel } from '../../src/components/AppearanceSettingsPanel';
import { loadAppearance, saveAppearance, resolveBaseTheme } from '../../src/appearance';
import '../../src/styles.css';

function Fixture() {
  const [appearance, setAppearance] = useState(loadAppearance);
  useLayoutEffect(() => {
    const root = document.documentElement;
    root.dataset.theme = resolveBaseTheme(appearance.theme);
    root.dataset.appearanceTheme = appearance.theme;
    root.dataset.appearanceAccent = appearance.accent;
    saveAppearance(appearance, resolveBaseTheme(appearance.theme));
  }, [appearance]);
  return <main className="settings-native settings-dialog" style={{ display: 'block', padding: 32, height: '100vh', overflow: 'auto' }}><AppearanceSettingsPanel appearance={appearance} onChange={setAppearance} /></main>;
}
createRoot(document.getElementById('root')!).render(<Fixture />);
