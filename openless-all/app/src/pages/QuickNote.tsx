import { useTranslation } from 'react-i18next';
import { ShortcutRecorder } from '../components/ShortcutRecorder';
import { isDesktop, setQuickNoteHotkey } from '../lib/ipc';
import { useHotkeySettings } from '../state/HotkeySettingsContext';
import { Card } from './_atoms';
import { History } from './History';

/** Quick notes share the unified history/actions surface but use permanent audio retention. */
export function QuickNote() {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12, height: '100%', minHeight: 0 }}>
      {isDesktop() && (
        <Card>
        <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 6 }}>
          {t('quickNote.shortcutTitle', 'Quick note shortcut')}
        </div>
        <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', marginBottom: 10 }}>
          {t(
            'quickNote.shortcutDesc',
            'Press once to start a permanent audio capture, then press again to finish.',
          )}
        </div>
        {prefs && (
          <ShortcutRecorder
            value={prefs.quickNoteHotkey}
            onSave={async (binding) => {
              await setQuickNoteHotkey(binding);
              await updatePrefs({ ...prefs, quickNoteHotkey: binding });
            }}
            onDisable={async () => {
              await setQuickNoteHotkey(null);
              await updatePrefs({ ...prefs, quickNoteHotkey: null });
            }}
          />
        )}
        </Card>
      )}
      <div style={{ flex: 1, minHeight: 0 }}>
        <History quickNotesOnly />
      </div>
    </div>
  );
}
