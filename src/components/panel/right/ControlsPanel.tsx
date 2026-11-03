import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { RotateCcw, Copy, ClipboardPaste, PencilSparkles, ChartArea, LayoutList } from 'lucide-react';
import { motion, AnimatePresence } from 'framer-motion';
import clsx from 'clsx';
import { useTranslation } from 'react-i18next';
import CollapsibleSection from '../../ui/CollapsibleSection';
import AdjustmentSection from '../../adjustments/AdjustmentSection';
import ConnectedWaveform from '../editor/ConnectedWaveform';
import Resizer from '../../ui/Resizer';
import AdjustmentSectionsSubMenu from './AdjustmentSectionsSubMenu';
import {
  ActiveTools,
  Adjustments,
  INITIAL_ADJUSTMENTS,
  ADJUSTMENT_SECTIONS,
  getActiveTools,
  getVisibleAdjustmentSections,
  isToolActive,
  setToolActive,
  toggleToolActive,
} from '../../../utils/adjustments';
import { useContextMenu } from '../../../context/ContextMenuContext';
import { OPTION_SEPARATOR, Orientation } from '../../ui/AppProperties';
import Text from '../../ui/Text';
import { TextVariants, TextColors, TextWeights } from '../../../types/typography';
import { useShallow } from 'zustand/react/shallow';
import { useEditorStore } from '../../../store/useEditorStore';
import { useSettingsStore } from '../../../store/useSettingsStore';
import { useUIStore } from '../../../store/useUIStore';
import { buildAutoAdjustModeOptions, useEditorActions } from '../../../hooks/useEditorActions';
import { useWaveformControls } from '../../../hooks/useWaveformControls';

const SECTION_IDS = ['basic', 'curves', 'color', 'details', 'effects'] as const;
type SectionId = (typeof SECTION_IDS)[number];

// The i18n resources are typed as a literal key union, so a template key built
// from a runtime string has to be narrowed back to the known section ids.
const sectionTitleKey = (sectionName: string) =>
  `editor.adjustments.sections.${sectionName}` as `editor.adjustments.sections.${SectionId}`;

function AdjustmentSectionContent(props: {
  sectionName: string;
  activeTools: ActiveTools;
  onToggleTool(id: string): void;
  [key: string]: unknown;
}) {
  const adjustments = useEditorStore((state) => state.adjustments);
  return <AdjustmentSection {...props} adjustments={adjustments} />;
}

export default function Controls({ compact = false }: { compact?: boolean } = {}) {
  const { t } = useTranslation();
  const { showContextMenu } = useContextMenu();
  const { isResizingWaveform, onToggleWaveform, setActiveWaveformChannel, handleWaveformResize } =
    useWaveformControls();
  const { setAdjustments, handleAutoAdjustments } = useEditorActions();

  const { appSettings, theme } = useSettingsStore(
    useShallow((state) => ({
      appSettings: state.appSettings,
      theme: state.theme,
    })),
  );

  const visibleSections = getVisibleAdjustmentSections(appSettings?.adjustmentLayout);

  // Compact (phone) layout shows one section at a time behind a chip row, like
  // a camera app's adjustment tabs — five stacked accordions are unusable in a
  // 300px-tall sheet. Desktop keeps the accordion stack.
  const [compactSection, setCompactSection] = useState<string>(() => visibleSections[0] ?? 'basic');
  const chipRowRef = useRef<HTMLDivElement | null>(null);

  // A settings change can hide the section that is selected. Resolve the
  // fallback during render instead of syncing it in an effect: an effect would
  // paint the now-hidden section for one frame first.
  const activeCompactSection = visibleSections.includes(compactSection)
    ? compactSection
    : (visibleSections[0] ?? 'basic');

  // Keep the selected chip visible as the row scrolls (no smooth behaviour on
  // purpose: the panel already animates, and OHOS webview janks on stacked
  // scroll animations).
  useEffect(() => {
    if (!compact) return;
    const row = chipRowRef.current;
    const chip = row?.querySelector<HTMLElement>('[aria-selected="true"]');
    if (!row || !chip) return;
    const left = chip.offsetLeft;
    const right = left + chip.offsetWidth;
    if (left < row.scrollLeft) {
      row.scrollLeft = Math.max(0, left - 8);
    } else if (right > row.scrollLeft + row.clientWidth) {
      row.scrollLeft = right - row.clientWidth + 8;
    }
  }, [compact, activeCompactSection]);

  const { collapsibleSectionsState, setUI } = useUIStore(
    useShallow((state) => ({
      collapsibleSectionsState: state.collapsibleSectionsState,
      setUI: state.setUI,
    })),
  );

  const {
    activeToolsState,
    sectionVisibility,
    copiedSectionAdjustments,
    selectedImage,
    isWbPickerActive,
    isWaveformVisible,
    waveformHeight,
    setEditor,
  } = useEditorStore(
    useShallow((state) => ({
      activeToolsState: state.adjustments.activeTools,
      sectionVisibility: state.adjustments.sectionVisibility,
      copiedSectionAdjustments: state.copiedSectionAdjustments,
      selectedImage: state.selectedImage,
      isWbPickerActive: state.isWbPickerActive,
      isWaveformVisible: state.isWaveformVisible,
      waveformHeight: state.waveformHeight,
      setEditor: state.setEditor,
    })),
  );

  const activeTools = useMemo(
    () => getActiveTools({ activeTools: activeToolsState, sectionVisibility }),
    [activeToolsState, sectionVisibility],
  );

  const setCopiedSectionAdjustments = useCallback(
    (val: any) => setEditor({ copiedSectionAdjustments: val }),
    [setEditor],
  );

  const toggleWbPicker = useCallback(
    () =>
      setEditor((state) => ({
        isWbPickerActive: !state.isWbPickerActive,
        isRelightPickerActive: false,
        activeCenterMarker: null,
      })),
    [setEditor],
  );

  const onDragStateChange = useCallback(
    (isDragging: boolean) => setEditor({ isSliderDragging: isDragging }),
    [setEditor],
  );

  const setCollapsibleState = useCallback(
    (updater: any) =>
      setUI((state) => ({
        collapsibleSectionsState: typeof updater === 'function' ? updater(state.collapsibleSectionsState) : updater,
      })),
    [setUI],
  );

  const handleToggleVisibility = useCallback(
    (id: string) => {
      setAdjustments((prev: Adjustments) => ({
        ...prev,
        activeTools: toggleToolActive(getActiveTools(prev), id),
      }));
    },
    [setAdjustments],
  );

  const handleResetAdjustments = () => {
    setAdjustments((prev: Adjustments) => ({
      ...prev,
      ...Object.keys(ADJUSTMENT_SECTIONS)
        .flatMap((s) => ADJUSTMENT_SECTIONS[s])
        .reduce((acc: any, key: string) => {
          acc[key] = INITIAL_ADJUSTMENTS[key as keyof Adjustments];
          return acc;
        }, {}),
      activeTools: { ...INITIAL_ADJUSTMENTS.activeTools },
    }));
  };

  const handleAutoAdjustContextMenu = (event: React.MouseEvent) => {
    event.preventDefault();
    event.stopPropagation();
    showContextMenu(event.clientX, event.clientY, buildAutoAdjustModeOptions(t, handleAutoAdjustments));
  };

  const handleToggleSection = (section: string) => {
    setCollapsibleState((prev: any) => {
      const isOpening = !prev[section];
      if (appSettings?.enableFocusMode && isOpening) {
        const newState = { ...prev };
        Object.keys(newState).forEach((key) => {
          newState[key] = false;
        });
        newState[section] = true;
        return newState;
      }
      return { ...prev, [section]: !prev[section] };
    });
  };

  const handleSectionContextMenu = (event: any, sectionName: string) => {
    event.preventDefault();
    event.stopPropagation();

    const sectionKeys = ADJUSTMENT_SECTIONS[sectionName];
    if (!sectionKeys) {
      return;
    }

    const handleCopy = () => {
      const { adjustments } = useEditorStore.getState();
      const adjustmentsToCopy: any = {};
      for (const key of sectionKeys) {
        if (Object.prototype.hasOwnProperty.call(adjustments, key)) {
          adjustmentsToCopy[key] = JSON.parse(JSON.stringify(adjustments[key as keyof Adjustments]));
        }
      }
      setCopiedSectionAdjustments({ section: sectionName, values: adjustmentsToCopy });
    };

    const handlePaste = () => {
      if (!copiedSectionAdjustments || copiedSectionAdjustments.section !== sectionName) {
        return;
      }
      setAdjustments((prev: Adjustments) => ({
        ...prev,
        ...copiedSectionAdjustments.values,
        activeTools: setToolActive(getActiveTools(prev), sectionName, true),
      }));
    };

    const handleReset = () => {
      const resetValues: any = {};
      for (const key of sectionKeys) {
        resetValues[key] = JSON.parse(JSON.stringify(INITIAL_ADJUSTMENTS[key as keyof Adjustments]));
      }
      setAdjustments((prev: Adjustments) => ({
        ...prev,
        ...resetValues,
        activeTools: setToolActive(getActiveTools(prev), sectionName, true),
      }));
    };

    const isPasteAllowed = copiedSectionAdjustments && copiedSectionAdjustments.section === sectionName;
    const translatedSection = t(sectionTitleKey(sectionName));

    const pasteLabel = copiedSectionAdjustments
      ? t('editor.adjustments.actions.pasteLabel', { section: translatedSection })
      : t('editor.adjustments.actions.pasteSettings');

    const options: any = [
      {
        label: t('editor.adjustments.actions.copySectionSettings', { section: translatedSection }),
        icon: Copy,
        onClick: handleCopy,
      },
      { label: pasteLabel, icon: ClipboardPaste, onClick: handlePaste, disabled: !isPasteAllowed },
      { type: OPTION_SEPARATOR },
      {
        label: t('editor.adjustments.actions.resetSectionSettings', { section: translatedSection }),
        icon: RotateCcw,
        onClick: handleReset,
      },
      { type: OPTION_SEPARATOR },
      {
        label: t('editor.adjustments.actions.customizePanels'),
        icon: LayoutList,
        submenu: [{ customComponent: AdjustmentSectionsSubMenu }],
      },
    ];

    showContextMenu(event.clientX, event.clientY, options);
  };

  const headerActions = (
    <div className="flex items-center gap-1">
      <button
        className="p-2 rounded-full hover:bg-surface disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
        disabled={!selectedImage}
        onClick={() => handleAutoAdjustments()}
        onContextMenu={handleAutoAdjustContextMenu}
        data-tooltip={t('editor.adjustments.tooltips.autoAdjust')}
      >
        <PencilSparkles size={18} />
      </button>
      <button
        className={clsx(
          'p-2 rounded-full transition-colors',
          isWaveformVisible ? 'bg-surface hover:bg-card-active' : 'hover:bg-surface',
        )}
        onClick={onToggleWaveform}
        data-tooltip={t('editor.adjustments.tooltips.toggleAnalytics')}
      >
        <ChartArea size={18} />
      </button>
      <button
        className="p-2 rounded-full hover:bg-surface disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
        disabled={!selectedImage}
        onClick={handleResetAdjustments}
        data-tooltip={t('editor.adjustments.tooltips.resetAdjustments')}
      >
        <RotateCcw size={18} />
      </button>
    </div>
  );

  const waveformBlock = (
    <AnimatePresence initial={false}>
      {isWaveformVisible && (
        <motion.div
          initial={{ height: 0, opacity: 0 }}
          animate={{ height: waveformHeight || 256, opacity: 1 }}
          exit={{ height: 0, opacity: 0 }}
          transition={{ duration: isResizingWaveform ? 0 : 0.2, ease: 'easeOut' }}
          className="shrink-0 flex flex-col relative border-b border-surface overflow-hidden"
        >
          <div className="grow w-full h-full p-3 pb-2 min-h-0">
            <ConnectedWaveform setDisplayMode={setActiveWaveformChannel} setAdjustments={setAdjustments} theme={theme} />
          </div>
          <Resizer direction={Orientation.Horizontal} onMouseDown={handleWaveformResize} />
        </motion.div>
      )}
    </AnimatePresence>
  );

  const sectionProps = {    activeTools,
    onToggleTool: handleToggleVisibility,
    setAdjustments,
    theme,
    appSettings,
    asShotWhiteBalance: selectedImage?.asShotWhiteBalance,
    isWbPickerActive,
    toggleWbPicker,
    onDragStateChange,
  };

  const noImagePlaceholder = (
    <div className="flex items-center justify-center h-full">
      <Text
        variant={TextVariants.heading}
        color={TextColors.secondary}
        weight={TextWeights.normal}
        className="text-center"
      >
        {t('editor.ai.noImageSelected')}
      </Text>
    </div>
  );

  // Phone: chips replace the accordion stack (one section visible at a time).
  // Long-press a chip for the section actions (copy/paste/reset/customize).
  if (compact) {
    return (
      <div className="flex flex-col h-full">
        <div className="flex items-center shrink-0 border-b border-surface pr-1">
          <div
            ref={chipRowRef}
            role="tablist"
            className="flex items-center gap-1 grow min-w-0 overflow-x-auto custom-scrollbar px-2 py-2"
          >
            {visibleSections.map((sectionName: string) => {
              const isSelected = activeCompactSection === sectionName;
              return (
                <button
                  key={sectionName}
                  role="tab"
                  aria-selected={isSelected}
                  onClick={() => setCompactSection(sectionName)}
                  onContextMenu={(e: any) => handleSectionContextMenu(e, sectionName)}
                  className={clsx(
                    'shrink-0 px-3 py-1.5 rounded-full text-xs font-medium whitespace-nowrap transition-colors',
                    isSelected ? 'bg-accent text-button-text' : 'bg-surface text-text-secondary',
                  )}
                >
                  {t(sectionTitleKey(sectionName))}
                </button>
              );
            })}
          </div>
          <div className="shrink-0 pl-1 ml-0.5 border-l border-surface">{headerActions}</div>
        </div>

        {waveformBlock}

        <div className="grow overflow-y-auto custom-scrollbar p-3">
          {selectedImage ? (
            <AdjustmentSectionContent sectionName={activeCompactSection} {...sectionProps} />
          ) : (
            noImagePlaceholder
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full">
      <div className="p-3 flex justify-between items-center shrink-0 border-b border-surface">
        <Text variant={TextVariants.title}>{t('editor.adjustments.title')}</Text>
        {headerActions}
      </div>

      {waveformBlock}

      <div className="grow overflow-y-scroll p-3 flex flex-col gap-2">
        {selectedImage ? (
          visibleSections.map((sectionName: string) => {
            const title = t(sectionTitleKey(sectionName));

            return (
              <div className="shrink-0 group" key={sectionName}>
                <CollapsibleSection
                  isContentVisible={isToolActive(activeTools, sectionName)}
                  isOpen={collapsibleSectionsState[sectionName as keyof typeof collapsibleSectionsState]}
                  onContextMenu={(e: any) => handleSectionContextMenu(e, sectionName)}
                  onToggle={() => handleToggleSection(sectionName)}
                  onToggleVisibility={() => handleToggleVisibility(sectionName)}
                  title={title}
                >
                  <AdjustmentSectionContent sectionName={sectionName} {...sectionProps} />
                </CollapsibleSection>
              </div>
            );
          })
        ) : (
          noImagePlaceholder
        )}
      </div>
    </div>
  );
}
