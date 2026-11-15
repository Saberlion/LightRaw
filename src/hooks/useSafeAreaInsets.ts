import { useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import { Invokes } from '../types/invokes';
import { useSettingsStore } from '../store/useSettingsStore';

interface OhosAvoidArea {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

/**
 * Safe-area insets for OHOS (phone layout G1). ArkWeb's
 * `env(safe-area-inset-*)` proved unreliable on this webview, so the values
 * come from the native avoid-area bridge instead: EntryAbility merges
 * `window.getWindowAvoidArea` (system/cutout/gesture/indicator rects) and
 * pushes physical px through the Rust bridge — initial value via the
 * `get_ohos_avoid_area` command, updates via the `ohos-avoid-area` event
 * (rotation, fold, keyboard). Here they are converted to CSS px (divided by
 * the webview's device pixel ratio) and written to the `--safe-*` custom
 * properties that the root layout and the phone tab shell consume.
 *
 * `styles.css` seeds the same variables with `env(safe-area-inset-*)`, so
 * non-OHOS platforms keep the standard fallback and this hook is a no-op
 * there.
 */
export function useSafeAreaInsets() {
  const isOhos = useSettingsStore((state) => state.isOhos);

  useEffect(() => {
    if (!isOhos) return;

    let active = true;
    const unlisteners: Array<() => void> = [];

    const apply = (insets: OhosAvoidArea) => {
      if (!active) return;
      // Physical px from the ArkTS side -> CSS px of this webview.
      const dpr = window.devicePixelRatio || 1;
      const root = document.documentElement.style;
      root.setProperty('--safe-top', `${Math.ceil(insets.top / dpr)}px`);
      root.setProperty('--safe-right', `${Math.ceil(insets.right / dpr)}px`);
      root.setProperty('--safe-bottom', `${Math.ceil(insets.bottom / dpr)}px`);
      root.setProperty('--safe-left', `${Math.ceil(insets.left / dpr)}px`);
    };

    invoke<OhosAvoidArea | null>(Invokes.GetOhosAvoidArea)
      .then((insets) => {
        if (insets) apply(insets);
      })
      .catch(() => undefined);

    listen<OhosAvoidArea>('ohos-avoid-area', (event) => apply(event.payload))
      .then((unlisten) => {
        if (active) unlisteners.push(unlisten);
        else unlisten();
      })
      .catch(() => undefined);

    return () => {
      active = false;
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, [isOhos]);
}
