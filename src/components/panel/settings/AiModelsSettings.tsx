import { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { ParseKeys, TFunction } from 'i18next';
import { invoke } from '@tauri-apps/api/core';
import clsx from 'clsx';
import { ChevronDown, ChevronRight, Package, RefreshCw } from 'lucide-react';
import Text from '../../ui/Text';
import { TextColors, TextVariants, TextWeights } from '../../../types/typography';
import { Invokes } from '../../../types/invokes';
import { formatBytes } from '../../../utils/formatBytes';

/**
 * Settings → AI model management (P1, read-only view).
 *
 * Lists every registry entry from `list_ai_models` in two sections — models
 * bundled with the app (OHOS release HAP rawfile) and download-on-demand
 * models — grouped by feature, with per-file status/size and the bundled /
 * sideload / GPU-variant annotations. Download and delete actions follow in
 * P2 (docs/AI_MODEL_MANAGEMENT_DESIGN.md).
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

interface ModelRowProps {
  model: AiModelInfo;
  t: TFunction;
}

const ModelRow = ({ model, t }: ModelRowProps) => {
  const sharedCount = SHARED_MASK_COUNTS[model.id];
  const variantNote = VARIANT_NOTES[model.id];
  return (
    <div className="py-2.5">
      <div className="flex items-center gap-3">
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
        </div>
        <Text
          variant={TextVariants.label}
          color={TextColors.primary}
          weight={TextWeights.medium}
          className="tabular-nums text-right shrink-0"
        >
          {model.sizeBytes > 0 ? formatBytes(model.sizeBytes, t, 1) : '—'}
        </Text>
      </div>
    </div>
  );
};

interface GroupProps {
  feature: AiFeature;
  models: AiModelInfo[];
  isCollapsed: boolean;
  onToggle(): void;
  t: TFunction;
}

const FeatureGroup = ({ feature, models, isCollapsed, onToggle, t }: GroupProps) => (
  <div className="pt-1.5 first:pt-0">
    <button
      onClick={onToggle}
      className="w-full flex items-center gap-1.5 py-1.5 text-left"
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
          <ModelRow key={model.id} model={model} t={t} />
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
  collapsed: Record<string, boolean>;
  onToggleGroup(key: string): void;
  t: TFunction;
}

const Section = ({ sectionKey, title, description, groups, collapsed, onToggleGroup, t }: SectionProps) => (
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
            key={group.feature}
            feature={group.feature}
            models={group.models}
            isCollapsed={!!collapsed[collapseKey]}
            onToggle={() => onToggleGroup(collapseKey)}
            t={t}
          />
        );
      })}
    </div>
  </div>
);

export default function AiModelsSettings() {
  const { t } = useTranslation();
  const [models, setModels] = useState<AiModelInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});

  const refresh = useCallback(async () => {
    setIsRefreshing(true);
    setError(null);
    try {
      setModels(await invoke<AiModelInfo[]>(Invokes.ListAiModels));
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

  const toggleGroup = (key: string) => setCollapsed((prev) => ({ ...prev, [key]: !prev[key] }));

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
              collapsed={collapsed}
              onToggleGroup={toggleGroup}
              t={t}
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
              collapsed={collapsed}
              onToggleGroup={toggleGroup}
              t={t}
            />
          )}
        </div>
      )}
    </div>
  );
}
