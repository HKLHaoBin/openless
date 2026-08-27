// 服务 → 网络：全局系统代理开关（issue #869）与 HTTP 连接超时（issue #998）。
// 关闭代理后所有 reqwest 请求直连；超时数字框失焦/回车才提交，避免逐键重建连接池。
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { Card } from '../_atoms';
import { SectionTitle, SettingRow, Toggle, inputStyle } from './shared';

const CONNECT_MIN = 5;
const CONNECT_MAX = 60;
const IDLE_MIN = 60;
const IDLE_MAX = 1800;
const REQUEST_MIN = 15;
const REQUEST_MAX = 300;

function clampInt(raw: string, min: number, max: number, fallback: number): number {
  const parsed = Number.parseInt(raw.trim(), 10);
  if (!Number.isFinite(parsed)) return fallback;
  return Math.min(max, Math.max(min, parsed));
}

export function NetworkSection() {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();
  const [connectDraft, setConnectDraft] = useState<string | null>(null);
  const [idleDraft, setIdleDraft] = useState<string | null>(null);
  const [requestDraft, setRequestDraft] = useState<string | null>(null);

  useEffect(() => {
    setConnectDraft(null);
    setIdleDraft(null);
    setRequestDraft(null);
  }, [
    prefs?.httpConnectTimeoutSecs,
    prefs?.httpPoolIdleTimeoutSecs,
    prefs?.httpRequestTimeoutSecs,
  ]);

  if (!prefs) return null;

  const commitConnect = () => {
    if (connectDraft === null) return;
    const next = clampInt(connectDraft, CONNECT_MIN, CONNECT_MAX, prefs.httpConnectTimeoutSecs);
    setConnectDraft(null);
    if (next !== prefs.httpConnectTimeoutSecs) {
      void updatePrefs(current => ({ ...current, httpConnectTimeoutSecs: next }));
    }
  };
  const commitIdle = () => {
    if (idleDraft === null) return;
    const next = clampInt(idleDraft, IDLE_MIN, IDLE_MAX, prefs.httpPoolIdleTimeoutSecs);
    setIdleDraft(null);
    if (next !== prefs.httpPoolIdleTimeoutSecs) {
      void updatePrefs(current => ({ ...current, httpPoolIdleTimeoutSecs: next }));
    }
  };
  const commitRequest = () => {
    if (requestDraft === null) return;
    const next = clampInt(requestDraft, REQUEST_MIN, REQUEST_MAX, prefs.httpRequestTimeoutSecs);
    setRequestDraft(null);
    if (next !== prefs.httpRequestTimeoutSecs) {
      void updatePrefs(current => ({ ...current, httpRequestTimeoutSecs: next }));
    }
  };

  return (
    <Card>
      <SectionTitle>{t('settings.network.title')}</SectionTitle>
      <SettingRow
        label={t('settings.network.useSystemProxyLabel')}
        desc={t('settings.network.useSystemProxyDesc')}
      >
        <Toggle
          on={prefs.useSystemProxy}
          onToggle={next =>
            void updatePrefs(current => ({ ...current, useSystemProxy: next }))
          }
        />
      </SettingRow>
      <SettingRow
        label={t('settings.network.connectTimeoutLabel')}
        desc={t('settings.network.connectTimeoutDesc')}
        controlWidth={100}
      >
        <input
          type="number"
          min={CONNECT_MIN}
          max={CONNECT_MAX}
          style={{ ...inputStyle, maxWidth: 100 }}
          value={connectDraft ?? String(prefs.httpConnectTimeoutSecs)}
          onChange={e => setConnectDraft(e.currentTarget.value)}
          onBlur={commitConnect}
          onKeyDown={e => {
            if (e.key === 'Enter') commitConnect();
          }}
        />
      </SettingRow>
      <SettingRow
        label={t('settings.network.poolIdleTimeoutLabel')}
        desc={t('settings.network.poolIdleTimeoutDesc')}
        controlWidth={100}
      >
        <input
          type="number"
          min={IDLE_MIN}
          max={IDLE_MAX}
          style={{ ...inputStyle, maxWidth: 100 }}
          value={idleDraft ?? String(prefs.httpPoolIdleTimeoutSecs)}
          onChange={e => setIdleDraft(e.currentTarget.value)}
          onBlur={commitIdle}
          onKeyDown={e => {
            if (e.key === 'Enter') commitIdle();
          }}
        />
      </SettingRow>
      <SettingRow
        label={t('settings.network.requestTimeoutLabel')}
        desc={t('settings.network.requestTimeoutDesc')}
        controlWidth={100}
      >
        <input
          type="number"
          min={REQUEST_MIN}
          max={REQUEST_MAX}
          style={{ ...inputStyle, maxWidth: 100 }}
          value={requestDraft ?? String(prefs.httpRequestTimeoutSecs)}
          onChange={e => setRequestDraft(e.currentTarget.value)}
          onBlur={commitRequest}
          onKeyDown={e => {
            if (e.key === 'Enter') commitRequest();
          }}
        />
      </SettingRow>
    </Card>
  );
}
