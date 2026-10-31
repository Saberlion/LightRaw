import { invoke } from '@tauri-apps/api/core';
import { Invokes } from '../components/ui/AppProperties';

/**
 * Staged album-import paths (OHOS). The MediaLibraryKit picker returns media
 * URIs, and the ArkTS side copies the selected assets into the app cache so the
 * regular import pipeline can copy them into the library. Those cache files are
 * removed once the import worker emits `import-complete`.
 */
let pendingPaths: string[] = [];

export function setOhosImportStaging(paths: string[]): void {
  pendingPaths = paths;
}

export async function cleanupOhosImportStaging(): Promise<void> {
  if (pendingPaths.length === 0) return;
  const paths = pendingPaths;
  pendingPaths = [];
  try {
    await invoke(Invokes.CleanupOhosImportStaging, { paths });
  } catch (err) {
    console.warn('Failed to clean the OHOS import staging directory:', err);
  }
}
