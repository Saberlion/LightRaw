import { useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { closeMergeModal, useUIStore } from '../store/useUIStore';
import { useLibraryStore } from '../store/useLibraryStore';
import { useSettingsStore } from '../store/useSettingsStore';

export function useAndroidBackHandler() {
  const isOhos = useSettingsStore((state) => state.isOhos);

  useEffect(() => {
    const osPlatform = useSettingsStore.getState().osPlatform;
    const isAndroid = osPlatform === 'android';
    // plugin-os reports "linux" on OHOS, so the Android guard alone would
    // never install the handler there: the OHOS back gesture arrives as the
    // `ohos-back-pressed` tauri event instead (MainPage → on_back_press_intercept).
    if (!isAndroid && !isOhos) return;

    const handleBack = () => {
      const ui = useUIStore.getState();
      const library = useLibraryStore.getState();
      // On-device audit line (mirrored into app.log; no devtools on real
      // devices): which view/state the back gesture met and which branch ate
      // it. TC-PHONE-15/20 chain evidence.
      console.info(
        `[ohos-back] view=${ui.activeView} fullscreen=${ui.isFullScreen} multi=${library.multiSelectedPaths.length} roots=${library.rootPaths?.length ?? 0}`,
      );

      if (ui.confirmModalState.isOpen) {
        ui.setUI((state: any) => ({ confirmModalState: { ...state.confirmModalState, isOpen: false } }));
        return;
      }
      if (ui.isCreateFolderModalOpen) {
        ui.setUI({ isCreateFolderModalOpen: false });
        return;
      }
      if (ui.isRenameFolderModalOpen) {
        ui.setUI({ isRenameFolderModalOpen: false });
        return;
      }
      if (ui.isRenameFileModalOpen) {
        ui.setUI({ isRenameFileModalOpen: false });
        return;
      }
      if (ui.isImportModalOpen) {
        ui.setUI({ isImportModalOpen: false });
        return;
      }
      if (ui.isCopyPasteSettingsModalOpen) {
        ui.setUI({ isCopyPasteSettingsModalOpen: false });
        return;
      }
      if (ui.isCreateAlbumModalOpen) {
        ui.setUI({ isCreateAlbumModalOpen: false });
        return;
      }
      if (ui.isCreateAlbumGroupModalOpen) {
        ui.setUI({ isCreateAlbumGroupModalOpen: false });
        return;
      }
      if (ui.isRenameAlbumModalOpen) {
        ui.setUI({ isRenameAlbumModalOpen: false });
        return;
      }
      if (ui.mergeModalState.isOpen) {
        closeMergeModal();
        return;
      }
      if (ui.cullingModalState.isOpen) {
        ui.setUI({
          cullingModalState: { isOpen: false, progress: null, suggestions: null, error: null, pathsToCull: [] },
        });
        return;
      }
      if (ui.collageModalState.isOpen) {
        ui.setUI({ collageModalState: { isOpen: false, sourceImages: [] } });
        return;
      }

      // Full-screen preview is the topmost overlay state: every other exit is
      // keyboard-only (F/Esc) or lives in the toolbar it hides, neither of
      // which exists on touch form factors — so the back gesture must leave
      // it before closing drawers or leaving the editor (TC-PHONE-20).
      if (ui.isFullScreen) {
        console.info('[ohos-back] -> exit full-screen preview');
        ui.toggleFullScreen();
        return;
      }

      // Touch form factors: the adjustments/folder drawer behaves like an
      // overlay, so the back gesture closes it before leaving the
      // library/editor (desktop panels are permanent columns and must not
      // respond to back). Phones only hit this in landscape / free windows,
      // where the same rail + drawer layout is used.
      const settings = useSettingsStore.getState();
      if (
        settings.isOhos &&
        (settings.ohosDeviceType === 'tablet' || settings.ohosDeviceType === 'phone') &&
        ui.uiVisibility.rightPanel &&
        window.innerWidth >= 700
      ) {
        ui.setUI((state: any) => ({ uiVisibility: { ...state.uiVisibility, rightPanel: false } }));
        return;
      }

      // Editor → library. Without this the OHOS back gesture fell through to
      // the ability default and terminated the app (D-25, TEST_REPORT_2026-11-04).
      // isFullScreen is reset defensively: it is otherwise only cleared inside
      // toggleFullScreen itself, so re-entering the editor would reopen into
      // the chrome-less preview with no touch way back out (TC-PHONE-20).
      if (ui.activeView === 'editor') {
        console.info('[ohos-back] -> editor -> library');
        ui.setUI({ activeView: 'library', slideDirection: 1, isFullScreen: false });
        return;
      }

      console.info('[ohos-back] -> dispatch synthetic Escape');
      window.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'Escape', code: 'Escape', bubbles: true, cancelable: true }),
      );
    };

    (window as any).__handleAndroidBack = handleBack;

    let active = true;
    let unlisten: (() => void) | undefined;
    if (isOhos) {
      listen('ohos-back-pressed', () => handleBack())
        .then((un) => {
          if (active) unlisten = un;
          else un();
        })
        .catch(() => undefined);
    }

    return () => {
      active = false;
      unlisten?.();
      delete (window as any).__handleAndroidBack;
    };
  }, [isOhos]);
}
