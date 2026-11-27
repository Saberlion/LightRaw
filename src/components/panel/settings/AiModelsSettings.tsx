import { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { ParseKeys, TFunction } from 'i18next';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import clsx from 'clsx';
import { useShallow } from 'zustand/react/shallow';
import { ChevronDown, ChevronRight, Download, Loader2, Package, RefreshCw, RotateCcw, Trash2 } from 'lucide-react';
import Text from '../../ui/Text';
import Switch from '../../ui/Switch';
import { TextColors, TextVariants, TextWeights } from '../../../types/typography';
import { Invokes } from '../../../types/invokes';
import type { AppSettings } from '../../../types/settings';
import { formatBytes } from '../../../utils/formatBytes';
import { isJobActive, JobKind, useProcessStore } from '../../../store/useProcessStore';
import { useSettingsStore } from '../../../store/useSettingsStore';
import type { DataConfirmRequest } from './DataManagementSettings';

/**
 * Settings → AI model management.
 *
 * Lists every registry entry from `list_ai_models` in two sections — models
 * bundled with the app (OHOS release HAP rawfile) and download-on-demand
 * models — grouped by feature, with per-file status/size and the bundled /
 * sideload / GPU-variant annotations. P2 adds per-model download (progress
 * rides the existing `ai-model-download-*` job events) and delete (confirm
 * variant depends on bundled/side-load semantics; AI sessions are dropped
 * backend-side so the next use re-initializes cleanly).
 * docs/AI_MODEL_MANAGEMENT_DESIGN.md
 */

type AiModelStatus = 'notInstalled' | 'ready' | 'corrupt' | 'bundled';

type AiFeature =
  | 'subjectMask'
  | 'foregroundMask'
  | 'sharedMask'
  | 'depthMask'
  | 'denoise'
  | 'upscale'
  | 'inpaint'
  | 'tagging'
  | 'normal';

interface AiModelInfo {
  id: string;
  name: string;
  filename: string;
  feature: AiFeature;
  status: AiModelStatus;
  sizeBytes: number;
  bundled: boolean;
  diskRequired: boolean;
  downloadable: boolean;
  sha256: string | null;
}

// Static key literals (not template strings) so i18next-cli extraction
// always finds them; typed as ParseKeys to satisfy i18next's typed keys.
const FEATURE_LABELS: Record<AiFeature, ParseKeys> = {
  subjectMask: 'settings.aiModels.feature.subjectMask',
  foregroundMask: 'settings.aiModels.feature.foregroundMask',
  sharedMask: 'settings.aiModels.feature.sharedMask',
  depthMask: 'settings.aiModels.feature.depthMask',
  denoise: 'settings.aiModels.feature.denoise',
  upscale: 'settings.aiModels.feature.upscale',
  inpaint: 'settings.aiModels.feature.inpaint',
  tagging: 'settings.aiModels.feature.tagging',
  normal: 'settings.aiModels.feature.normal',
};

// Registry ids shared by more than one mask feature (backend does not
// serialize the mask-model mapping; these are the known multi-consumers).
const SHARED_MASK_COUNTS: Record<string, number> = {
  esam3_image_encoder: 3,
  esam3_decoder: 3,
};

// BiRefNet ships two weight variants; exactly one is active depending on
// the GPU-acceleration setting.
const VARIANT_NOTES: Record<string, ParseKeys> = {
  birefnet_fp16: 'settings.aiModels.gpuVariantNote',
  birefnet_fp32: 'settings.aiModels.cpuVariantNote',
};

function describeStatus(model: AiModelInfo, t: TFunction): string {
  switch (model.status) {
    case 'ready':
      // Bundled + extracted to disk (CANN/NNRT artifacts always need a
      // filesystem path); a bundled ONNX with a disk copy is a hot-update
      // override and just reports ready.
      return model.bundled && model.diskRequired
        ? t('settings.aiModels.bundledExtracted', { size: formatBytes(model.sizeBytes, t, 1) })
        : t('settings.aiModels.status.ready');
    case 'bundled':
      return model.diskRequired
        ? t('settings.aiModels.bundledWillExtract', { size: formatBytes(model.sizeBytes, t, 1) })
        : t('settings.aiModels.bundledReady');
    case 'corrupt':
      return t('settings.aiModels.corruptNote');
    default:
      return model.downloadable ? t('settings.aiModels.status.notInstalled') : t('settings.aiModels.sideloadNote');
  }
}

function statusColor(model: AiModelInfo): string | undefined {
  switch (model.status) {
    case 'ready':
    case 'bundled':
      return 'text-green-500';
    case 'corrupt':
      return 'text-red-400';
    default:
      return undefined;
  }
}

interface AiModelsSettingsProps {
  onRequestConfirm(request: DataConfirmRequest): void;
  appSettings: AppSettings | null;
  onSettingsChange(settings: AppSettings): void | Promise<void>;
}

type AutoDownloadState = 'disabled' | 'blockedNetwork' | 'blockedBusy' | 'waitingIdle' | 'upToDate' | 'running';

interface AutoDownloadStatus {
  state: AutoDownloadState;
  done: number;
  total: number;
  current: string | null;
}

const AUTO_STATUS_KEYS: Record<AutoDownloadState, ParseKeys> = {
  disabled: 'settings.aiModels.autoStatus.waitingIdle',
  blockedNetwork: 'settings.aiModels.autoStatus.blockedNetwork',
  blockedBusy: 'settings.aiModels.autoStatus.blockedBusy',
  waitingIdle: 'settings.aiModels.autoStatus.waitingIdle',
  upToDate: 'settings.aiModels.autoStatus.upToDate',
  running: 'settings.aiModels.autoStatus.running',
};

function describeAutoStatus(status: AutoDownloadStatus, t: TFunction): string {
  switch (status.state) {
    case 'running':
      return t('settings.aiModels.autoStatus.running', {
        name: status.current ?? '',
        done: status.done,
        total: status.total,
      });
    case 'waitingIdle':
      return t('settings.aiModels.autoStatus.waitingIdle', { count: status.total });
    default:
      return t(AUTO_STATUS_KEYS[status.state]);
  }
}

interface RowContext {
  canDownload: boolean;
  aiTasksRunning: boolean;
  onDelete(model: AiModelInfo): void;
  onDownload(model: AiModelInfo, force: boolean): void;
  pending: Record<string, 'download' | 'delete'>;
  errors: Record<string, string | null>;
  progress: Record<string, { current?: number; total: number }>;
  t: TFunction;
}

function RowActionButton({
  danger = false,
  disabled = false,
  icon,
  label,
  onClick,
  tooltip,
}: {
  danger?: boolean;
  disabled?: boolean;
  icon: React.ReactNode;
  label: string;
  onClick(): void;
  tooltip?: string;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      data-tooltip={tooltip}
      className={clsx(
        'flex items-center gap-1.5 h-7 px-2.5 rounded-md text-xs font-medium shrink-0 transition-colors',
        'text-text-secondary bg-bg-primary border border-border-color',
        'disabled:opacity-40 disabled:cursor-not-allowed',
        danger
          ? 'enabled:hover:text-red-400 enabled:hover:border-red-400/40 enabled:hover:bg-red-500/10'
          : 'enabled:hover:text-text-primary enabled:hover:border-text-secondary/40',
      )}
      style={{ WebkitTapHighlightColor: 'transparent' }}
    >
      {icon}
      {label}
    </button>
  );
}

function RowActions({ model, ctx }: { model: AiModelInfo; ctx: RowContext }) {
  const { t, progress } = ctx;
  const active = progress[model.name];
  const pendingOp = ctx.pending[model.id];

  if (active || pendingOp === 'download') {
    const current = active?.current;
    const total = active?.total ?? 0;
    return (
      <div className="flex items-center gap-2 shrink-0" style={{ minWidth: 88 }}>
        {current !== undefined && total > 0 ? (
          <>
            <div className="flex-1 h-1 rounded-full bg-bg-primary overflow-hidden">
              <div
                className="h-full rounded-full bg-accent transition-[width] duration-200"
                style={{ width: `${Math.round((current / total) * 100)}%` }}
              />
            </div>
            <Text variant={TextVariants.small} className="tabular-nums shrink-0">
              {Math.round((current / total) * 100)}%
            </Text>
          </>
        ) : (
          <Loader2 size={13} className="animate-spin text-text-secondary" />
        )}
      </div>
    );
  }
  if (pendingOp === 'delete') {
    return <Loader2 size={13} className="animate-spin text-text-secondary shrink-0" />;
  }

  const onDisk = model.status === 'ready' || model.status === 'corrupt';
  const downloadDisabled = !ctx.canDownload;
  const deleteDisabled = ctx.aiTasksRunning;
  const deleteLabelKey: ParseKeys =
    model.bundled && model.diskRequired ? 'settings.aiModels.reclaimAction' : 'settings.aiModels.actions.delete';

  return (
    <div className="flex justify-end gap-1.5 shrink-0">
      {model.downloadable && model.status !== 'ready' && (
        <RowActionButton
          disabled={downloadDisabled}
          icon={<Download size={13} />}
          label={t('settings.aiModels.actions.download')}
          onClick={() => ctx.onDownload(model, false)}
        />
      )}
      {model.downloadable && model.status === 'ready' && (
        <RowActionButton
          disabled={downloadDisabled}
          icon={<RotateCcw size={13} />}
          label={t('settings.aiModels.actions.redownload')}
          onClick={() => ctx.onDownload(model, true)}
        />
      )}
      {onDisk && (
        <RowActionButton
          danger
          disabled={deleteDisabled}
          icon={<Trash2 size={13} />}
          label={t(deleteLabelKey)}
          onClick={() => ctx.onDelete(model)}
          tooltip={deleteDisabled ? t('settings.aiModels.busyAiTask') : undefined}
        />
      )}
    </div>
  );
}

const ModelRow = ({ model, ctx, t }: { model: AiModelInfo; ctx: RowContext; t: TFunction }) => {
  const sharedCount = SHARED_MASK_COUNTS[model.id];
  const variantNote = VARIANT_NOTES[model.id];
  const error = ctx.errors[model.id];
  return (
    <div className="flex items-center gap-3 py-2.5">
      {model.bundled && (
        <span
          data-tooltip={model.filename}
          className="flex items-center gap-1 px-1.5 h-6 rounded-md text-[10px] font-medium text-accent bg-accent/10 border border-accent/30 shrink-0"
        >
          <Package size={11} />
          {t('settings.aiModels.bundledBadge')}
        </span>
      )}
      <div className="flex-1 min-w-0">
        <Text
          variant={TextVariants.label}
          color={TextColors.primary}
          weight={TextWeights.medium}
          className="block truncate"
        >
          {model.name}
        </Text>
        <Text
          variant={TextVariants.small}
          color={statusColor(model) === undefined ? TextColors.secondary : undefined}
          className={clsx('mt-0.5 truncate', statusColor(model))}
        >
          {model.filename} · {describeStatus(model, t)}
        </Text>
        {(sharedCount !== undefined || variantNote !== undefined) && (
          <Text variant={TextVariants.small} color={TextColors.secondary} className="mt-0.5">
            {sharedCount !== undefined ? t('settings.aiModels.sharedNote', { count: sharedCount }) : null}
            {sharedCount !== undefined && variantNote !== undefined ? ' · ' : null}
            {variantNote !== undefined ? t(variantNote) : null}
          </Text>
        )}
        {error && (
          <Text variant={TextVariants.small} color={TextColors.error} className="mt-0.5">
            {error}
          </Text>
        )}
      </div>
      <Text
        variant={TextVariants.label}
        color={TextColors.primary}
        weight={TextWeights.medium}
        className="tabular-nums text-right shrink-0"
      >
        {model.sizeBytes > 0 ? formatBytes(model.sizeBytes, t, 1) : '—'}
      </Text>
      <RowActions model={model} ctx={ctx} />
    </div>
  );
};

interface GroupProps {
  feature: AiFeature;
  models: AiModelInfo[];
  ctx: RowContext;
  t: TFunction;
  isCollapsed: boolean;
  onToggle(): void;
}

const FeatureGroup = ({ feature, models, ctx, t, isCollapsed, onToggle }: GroupProps) => (
  <div>
    <button
      onClick={onToggle}
      className="w-full flex items-center gap-1.5 py-1 text-left"
      style={{ WebkitTapHighlightColor: 'transparent' }}
    >
      <Text color={TextColors.secondary} className="shrink-0">
        {isCollapsed ? <ChevronRight size={13} /> : <ChevronDown size={13} />}
      </Text>
      <Text variant={TextVariants.label} color={TextColors.primary} weight={TextWeights.medium}>
        {t(FEATURE_LABELS[feature])}
      </Text>
      <Text variant={TextVariants.small} color={TextColors.secondary}>
        ({models.length})
      </Text>
    </button>
    {!isCollapsed && (
      <div className="pl-5 divide-y divide-border-color/60">
        {models.map((model) => (
          <ModelRow key={model.id} model={model} ctx={ctx} t={t} />
        ))}
      </div>
    )}
  </div>
);

interface SectionProps {
  sectionKey: 'bundled' | 'downloadable';
  title: string;
  description: string;
  groups: Array<{ feature: AiFeature; models: AiModelInfo[] }>;
  ctx: RowContext;
  t: TFunction;
  collapsed: Record<string, boolean>;
  onToggleGroup(key: string): void;
}

const Section = ({ sectionKey, title, description, groups, ctx, t, collapsed, onToggleGroup }: SectionProps) => (
  <div>
    <Text variant={TextVariants.heading} className="mb-1">
      {title}
    </Text>
    <Text variant={TextVariants.small} className="mb-2">
      {description}
    </Text>
    <div className="divide-y divide-border-color/60">
      {groups.map((group) => {
        // The same feature can appear in both sections (e.g. denoise ONNX
        // downloads while the bundled .om/.ms live in the built-in section),
        // so collapse state is keyed per section+feature.
        const collapseKey = `${sectionKey}:${group.feature}`;
        return (
          <FeatureGroup
            key={collapseKey}
            feature={group.feature}
            models={group.models}
            ctx={ctx}
            t={t}
            isCollapsed={!!collapsed[collapseKey]}
            onToggle={() => onToggleGroup(collapseKey)}
          />
        );
      })}
    </div>
  </div>
);

export default function AiModelsSettings({ onRequestConfirm, appSettings, onSettingsChange }: AiModelsSettingsProps) {
  const { t } = useTranslation();
  const isOhos = useSettingsStore((s) => s.isOhos);
  const [models, setModels] = useState<AiModelInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [pending, setPending] = useState<Record<string, 'download' | 'delete'>>({});
  const [errors, setErrors] = useState<Record<string, string | null>>({});
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const [autoStatus, setAutoStatus] = useState<AutoDownloadStatus | null>(null);

  // Idle auto-download scheduler status: seed once, then follow the
  // `ai-auto-download` events (the page is not always open when the
  // scheduler ticks, so the command doubles as event replay).
  useEffect(() => {
    if (!isOhos) return;
    let disposed = false;
    invoke<AutoDownloadStatus>(Invokes.GetAutoDownloadStatus)
      .then((status) => {
        if (!disposed) setAutoStatus(status);
      })
      .catch((err) => console.error('Failed to get auto-download status:', err));
    const unlisten = listen<AutoDownloadStatus>('ai-auto-download', (event) => {
      setAutoStatus(event.payload);
    });
    return () => {
      disposed = true;
      unlisten.then((dispose) => dispose()).catch(() => undefined);
    };
  }, [isOhos]);

  // Live download progress from the global job store: ModelDownload jobs
  // keyed by the model's display name (event payload), unique since the
  // registry disambiguated the BiRefNet variant names.
  const progress = useProcessStore(
    useShallow((state) => {
      const map: Record<string, { current?: number; total: number }> = {};
      for (const job of Object.values(state.jobs)) {
        if (job.kind === JobKind.ModelDownload && isJobActive(job.status) && job.detail) {
          map[job.detail] = job.progress;
        }
      }
      return map;
    }),
  );
  const activeProgress = useProcessStore(
    useShallow(
      (state) =>
        Object.values(state.jobs).filter((job) => job.kind === JobKind.ModelDownload && isJobActive(job.status)).length,
    ),
  );
  const aiTasksRunning = useProcessStore(useShallow((state) => Object.keys(state.activeAiTasks).length > 0));
  const hasPendingDownload = Object.values(pending).includes('download');

  const refresh = useCallback(async () => {
    setIsRefreshing(true);
    setError(null);
    try {
      setModels(await invoke<AiModelInfo[]>(Invokes.ListAiModels));
      setErrors({});
    } catch (err) {
      console.error('Failed to list AI models:', err);
      setError(String(err));
    } finally {
      setIsRefreshing(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const clearPending = (id: string) =>
    setPending((prev) => {
      const next = { ...prev };
      delete next[id];
      return next;
    });

  const handleDownload = useCallback(
    async (model: AiModelInfo, force: boolean) => {
      setPending((prev) => ({ ...prev, [model.id]: 'download' }));
      try {
        await invoke(Invokes.DownloadAiModel, { id: model.id, force });
        setErrors((prev) => ({ ...prev, [model.id]: null }));
      } catch (err) {
        console.error('Model download failed:', err);
        setErrors((prev) => ({
          ...prev,
          [model.id]: t('settings.aiModels.downloadFailed', { error: String(err) }),
        }));
      } finally {
        clearPending(model.id);
        refresh();
      }
    },
    [refresh, t],
  );

  const runDelete = useCallback(
    async (model: AiModelInfo) => {
      setPending((prev) => ({ ...prev, [model.id]: 'delete' }));
      try {
        await invoke(Invokes.DeleteAiModel, { id: model.id });
        setErrors((prev) => ({ ...prev, [model.id]: null }));
      } catch (err) {
        console.error('Model delete failed:', err);
        setErrors((prev) => ({
          ...prev,
          [model.id]: t('settings.aiModels.deleteFailed', { error: String(err) }),
        }));
      } finally {
        clearPending(model.id);
        refresh();
      }
    },
    [refresh, t],
  );

  const requestDeleteConfirm = useCallback(
    (model: AiModelInfo) => {
      const name = model.name;
      const size = formatBytes(model.sizeBytes, t, 1);
      let message: string;
      let confirmText: string;
      if (!model.downloadable) {
        // Side-loaded SoC-specific .om: deleting it removes NPU support.
        message = t('settings.aiModels.deleteConfirm.npu', { name });
        confirmText = t('settings.aiModels.actions.delete');
      } else if (model.bundled && model.diskRequired) {
        // Extracted copy of a bundled .om/.ms — reclaim disk space, the
        // bundle re-extracts on next use.
        message = t('settings.aiModels.deleteConfirm.reclaim', { name, size });
        confirmText = t('settings.aiModels.reclaimAction');
      } else if (model.bundled) {
        // Disk override of a bundled model — falls back to the HAP copy.
        message = t('settings.aiModels.deleteConfirm.fallback', { name });
        confirmText = t('settings.aiModels.actions.delete');
      } else {
        message = t('settings.aiModels.deleteConfirm.message', { name, size });
        confirmText = t('settings.aiModels.actions.delete');
      }
      onRequestConfirm({
        title: t('settings.aiModels.deleteConfirm.title'),
        message,
        confirmText,
        onConfirm: () => runDelete(model),
      });
    },
    [onRequestConfirm, runDelete, t],
  );

  const toggleGroup = useCallback((key: string) => setCollapsed((prev) => ({ ...prev, [key]: !prev[key] })), []);

  const ctx: RowContext = {
    canDownload: activeProgress === 0 && !hasPendingDownload,
    aiTasksRunning,
    onDelete: requestDeleteConfirm,
    onDownload: handleDownload,
    pending,
    errors,
    progress,
    t,
  };

  const groupByFeature = (list: AiModelInfo[]) => {
    // Preserve the registry (backend) order: first appearance defines the
    // group order, rows inside a group keep their relative order.
    const order: AiFeature[] = [];
    const byFeature = new Map<AiFeature, AiModelInfo[]>();
    for (const model of list) {
      if (!byFeature.has(model.feature)) {
        byFeature.set(model.feature, []);
        order.push(model.feature);
      }
      byFeature.get(model.feature)!.push(model);
    }
    return order.map((feature) => ({ feature, models: byFeature.get(feature)! }));
  };

  const bundledModels = models?.filter((model) => model.bundled) ?? [];
  const downloadableModels = models?.filter((model) => !model.bundled) ?? [];
  const bundledTotal = bundledModels.reduce((sum, model) => sum + model.sizeBytes, 0);
  const downloadableDisk = downloadableModels
    .filter((model) => model.status === 'ready' || model.status === 'corrupt')
    .reduce((sum, model) => sum + model.sizeBytes, 0);

  return (
    <div>
      <div className="flex items-start justify-between gap-4 mb-3">
        <div>
          <Text variant={TextVariants.heading} className="mb-2">
            {t('settings.aiModels.title')}
          </Text>
          <Text variant={TextVariants.small}>{t('settings.aiModels.desc')}</Text>
        </div>
        <button
          onClick={refresh}
          disabled={isRefreshing}
          data-tooltip={t('settings.aiModels.refresh')}
          className="p-1.5 rounded-md text-text-secondary hover:text-text-primary hover:bg-bg-primary transition-colors"
        >
          <RefreshCw size={14} className={clsx(isRefreshing && 'animate-spin')} />
        </button>
      </div>

      {isOhos && (
        <div className="mb-4">
          <Switch
            checked={appSettings?.autoDownloadModels ?? true}
            id="auto-download-models-toggle"
            label={t('settings.aiModels.autoDownload')}
            onChange={(checked) => onSettingsChange({ ...appSettings, autoDownloadModels: checked } as AppSettings)}
          />
          <Text variant={TextVariants.small} color={TextColors.secondary} className="mt-1.5">
            {t('settings.aiModels.autoDownloadDesc')}
          </Text>
          {autoStatus && (appSettings?.autoDownloadModels ?? true) && (
            <Text
              variant={TextVariants.small}
              color={autoStatus.state === 'running' ? TextColors.success : undefined}
              className="mt-0.5"
            >
              {describeAutoStatus(autoStatus, t)}
            </Text>
          )}
        </div>
      )}

      {models === null && !error && (
        <Text variant={TextVariants.small} color={TextColors.secondary}>
          {t('settings.aiModels.status.checking')}
        </Text>
      )}
      {error && (
        <Text variant={TextVariants.small} color={TextColors.error}>
          {t('settings.aiModels.loadFailed')} {error}
        </Text>
      )}

      {models !== null && (
        <div className="space-y-8">
          {bundledModels.length > 0 && (
            <Section
              sectionKey="bundled"
              title={t('settings.aiModels.section.bundled')}
              description={t('settings.aiModels.section.bundledDesc', {
                size: formatBytes(bundledTotal, t, 1),
              })}
              groups={groupByFeature(bundledModels)}
              ctx={ctx}
              t={t}
              collapsed={collapsed}
              onToggleGroup={toggleGroup}
            />
          )}
          {downloadableModels.length > 0 && (
            <Section
              sectionKey="downloadable"
              title={t('settings.aiModels.section.downloadable')}
              description={t('settings.aiModels.section.downloadableDesc', {
                size: formatBytes(downloadableDisk, t, 1),
              })}
              groups={groupByFeature(downloadableModels)}
              ctx={ctx}
              t={t}
              collapsed={collapsed}
              onToggleGroup={toggleGroup}
            />
          )}
        </div>
      )}
    </div>
  );
}
