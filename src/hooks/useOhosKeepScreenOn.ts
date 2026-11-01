import { useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

import { Invokes } from '../components/ui/AppProperties';
import { Status } from '../components/ui/ExportImportProperties';
import { isJobActive, useProcessStore } from '../store/useProcessStore';
import { useSettingsStore } from '../store/useSettingsStore';

/**
 * HarmonyOS locks the screen on its own timeout, and a locked/backgrounded app
 * can be killed mid-job (observed: `AMS Kill process` during a 100 MP AI
 * denoise). Long-running work therefore holds the screen awake for its whole
 * duration and releases it as soon as nothing is running.
 *
 * Covered work: export/import, the background job registry (copy, move, model
 * download, AI tagging) and the AI job runner (denoise, depth/normal maps,
 * masks). The command is a no-op outside OHOS builds.
 */
const isLongJobRunning = (state: ReturnType<typeof useProcessStore.getState>) => {
  if (state.exportState.status === Status.Exporting || state.importState.status === Status.Importing) {
    return true;
  }
  if (Object.values(state.jobs).some((job) => isJobActive(job.status))) {
    return true;
  }
  if (state.aiJobs.current !== null || state.aiJobs.queued > 0) {
    return true;
  }
  return Object.keys(state.activeAiTasks).length > 0;
};

export function useOhosKeepScreenOn() {
  const isOhos = useSettingsStore((state) => state.isOhos);
  const isBusy = useProcessStore(isLongJobRunning);

  useEffect(() => {
    if (!isOhos) return;
    console.info(`[ohos-keep-screen] ${isBusy ? 'hold' : 'release'}`);
    invoke(Invokes.SetOhosKeepScreenOn, { enabled: isBusy }).catch((error) => {
      console.error(`Failed to set OHOS keep-screen-on (${isBusy}):`, error);
    });
  }, [isBusy, isOhos]);

  // Release the hold when the app unmounts (hot reload, window teardown) so a
  // stale flag cannot keep the display awake.
  useEffect(() => {
    if (!isOhos) return;
    return () => {
      invoke(Invokes.SetOhosKeepScreenOn, { enabled: false }).catch(() => {});
    };
  }, [isOhos]);
}
