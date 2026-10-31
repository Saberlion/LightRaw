import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import { platform } from '@tauri-apps/plugin-os';
import { AppSettings, SupportedTypes, Invokes } from '../components/ui/AppProperties';
import { DEFAULT_THEME_ID } from '../utils/themes';
import type { OhosPrivacyState } from '../utils/ohosPrivacy';

/**
 * Ask the ArkTS bridge for the device form factor ("phone" | "tablet" | "2in1").
 * The native module import can resolve after the webview has already booted, so
 * a null answer is retried a few times (an `ohos-device-type` event covers the
 * same race from the other side).
 */
const pollOhosDeviceType = (attempt = 0) => {
  invoke<string | null>(Invokes.GetOhosDeviceType)
    .then((deviceType) => {
      if (deviceType) {
        useSettingsStore.setState({ ohosDeviceType: deviceType });
        return;
      }
      if (attempt < 4) {
        window.setTimeout(() => pollOhosDeviceType(attempt + 1), 200 * (attempt + 1));
      } else {
        // The bridge never answered (missing export in an older HAP): settle on
        // "unknown" so the UI does not wait forever; a late event, if any, still
        // overwrites this with the real form factor.
        useSettingsStore.setState({ ohosDeviceType: 'unknown' });
      }
    })
    .catch(() => useSettingsStore.setState({ ohosDeviceType: 'unknown' }));
};

interface SettingsState {
  appSettings: AppSettings | null;
  theme: string;
  supportedTypes: SupportedTypes | null;
  osPlatform: string;
  isOhos: boolean;
  /** Whether the IsOhosBuild probe has settled (true + false = known not OHOS). */
  isOhosKnown: boolean;
  /** OHOS form factor from ArkTS `deviceInfo.deviceType` (null until known). */
  ohosDeviceType: string | null;
  isSystemDark: boolean | null;
  /** Last AppGallery privacy payload (OHOS only; null elsewhere). */
  ohosPrivacy: OhosPrivacyState | null;

  // Actions
  initPlatform: () => void;
  setAppSettings: (settings: AppSettings | null) => void;
  setTheme: (theme: string) => void;
  setSupportedTypes: (types: SupportedTypes | null) => void;
  setSystemDark: (dark: boolean) => void;
  setOhosPrivacy: (state: OhosPrivacyState | null) => void;
  setOhosDeviceType: (deviceType: string | null) => void;
  handleSettingsChange: (newSettings: AppSettings) => Promise<void>;
}

export const useSettingsStore = create<SettingsState>((set, get) => ({
  appSettings: null,
  theme: DEFAULT_THEME_ID,
  supportedTypes: null,
  osPlatform: '',
  isOhos: false,
  isOhosKnown: false,
  ohosDeviceType: null,
  isSystemDark: null,
  ohosPrivacy: null,

  initPlatform: () => {
    try {
      set({ osPlatform: platform() });
    } catch (_err) {
      set({ osPlatform: '' });
    }
    // plugin-os reports "linux" on OpenHarmony; detect OHOS through the Rust
    // bridge instead (same pattern as the TitleBar window controls).
    invoke<boolean>(Invokes.IsOhosBuild)
      .then((ohos) => {
        set({ isOhos: ohos, isOhosKnown: true });
        if (ohos) pollOhosDeviceType();
      })
      .catch(() => set({ isOhos: false, isOhosKnown: true }));
  },

  setAppSettings: (settings) => set({ appSettings: settings }),

  setTheme: (theme) => set({ theme }),

  setSupportedTypes: (types) => set({ supportedTypes: types }),

  setSystemDark: (dark) => set({ isSystemDark: dark }),

  setOhosPrivacy: (state) => set({ ohosPrivacy: state }),

  setOhosDeviceType: (deviceType) => set({ ohosDeviceType: deviceType }),

  handleSettingsChange: async (newSettings: AppSettings) => {
    if (!newSettings) {
      console.error('handleSettingsChange was called with null settings. Aborting save operation.');
      return;
    }

    if (newSettings.theme && newSettings.theme !== get().theme) {
      set({ theme: newSettings.theme });
    }

    const { searchCriteria: _searchCriteria, ...settingsToSave } = newSettings as any;
    set({ appSettings: newSettings });

    try {
      await invoke(Invokes.SaveSettings, { settings: settingsToSave });
    } catch (err) {
      console.error('Failed to save settings:', err);
    }
  },
}));
