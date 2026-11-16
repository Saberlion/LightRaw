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
  windowHeight: number;
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
 * ArkUI's own safe-area handling may ALREADY inset the webview below the
 * system bars (its default): then the viewport height times the device pixel
 * ratio falls short of the window-rect height by roughly the inset sum, and
 * padding again would double the safe area — exactly the dead top band the
 * real-device feedback caught. The window height rides along on the bridge
 * so this hook can detect that case and zero the CSS variables instead.
 */
export function useSafeAreaInsets() {
  const isOhos = useSettingsStore((state) => state.isOhos);

  useEffect(() => {
    if (!isOhos) return;

    let active = true;
    const unlisteners: Array<() => void> = [];

    const apply = (insets: OhosAvoidArea) => {
      if (!active) return;
      const dpr = window.devicePixelRatio || 1;
      const root = document.documentElement.style;

      // Did the system already inset this webview? Compare the physical
      // viewport height with the physical window-rect height: a webview laid
      // out inside the safe area falls short by about the inset sum.
      const viewportPhysical = Math.round(window.innerHeight * dpr);
      const missing = Math.max(0, insets.windowHeight - viewportPhysical);
      const insetSum = insets.top + insets.bottom;
      // Tolerance 4px: the CSS viewport rounds at dpr granularity (a 1 CSS px
      // rounding error is ~3.5 physical px on a 3.5 dpr screen).
      const alreadyInset = insetSum > 0 && missing + 4 >= insetSum;

      const css = alreadyInset
        ? { top: 0, right: 0, bottom: 0, left: 0 }
        : {
            top: Math.ceil(insets.top / dpr),
            right: Math.ceil(insets.right / dpr),
            bottom: Math.ceil(insets.bottom / dpr),
            left: Math.ceil(insets.left / dpr),
          };

      // On-device evidence for the avoid-area bridge (app.log/console chains
      // are the only observability a store-less webview has here).
      console.info(
        `[ohos-safe-area] top=${css.top}px right=${css.right}px bottom=${css.bottom}px left=${css.left}px` +
          ` (physical ${insets.top}/${insets.right}/${insets.bottom}/${insets.left} @ dpr ${dpr},` +
          ` window ${insets.windowHeight}px vs viewport ${viewportPhysical}px${alreadyInset ? ', system already inset -> zeroed' : ''})`,
      );
      root.setProperty('--safe-top', `${css.top}px`);
      root.setProperty('--safe-right', `${css.right}px`);
      root.setProperty('--safe-bottom', `${css.bottom}px`);
      root.setProperty('--safe-left', `${css.left}px`);
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
