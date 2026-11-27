import { useCallback } from 'react';
import { useEditorStore } from '../../../store/useEditorStore';
import { useEditorActions } from '../../../hooks/useEditorActions';
import LUTControl from '../../adjustments/LUTControl';
import LutIcon from '../../ui/LutIcon';
import Text from '../../ui/Text';
import { TextVariants } from '../../../types/typography';
import { Adjustments } from '../../../utils/adjustments';

/**
 * Standalone LUT panel (its own rail slot, right before Presets).
 *
 * FORK-OWNED UI, decoupled from upstream's LUT placement (docs/
 * HARMONYOS_PORTING.md §6.44): upstream RapidRAW embeds the LUT picker as a
 * sub-section of the Effects panel; this fork keeps upstream's Effects code
 * untouched (so upstream merges stay conflict-free) and instead ships this
 * dedicated rail panel as the visible surface. The upstream Effects entry is
 * hidden by default via DEFAULT_HIDDEN_ADJUSTMENT_TOOLS and stays
 * user-reenableable from the section submenu — both placements evolve
 * independently and render the same shared LUTControl.
 *
 * LUTs are looks — they lived buried in Effects > LUT, then as a header block
 * inside Presets. As a dedicated panel the picker is one rail tap away and
 * the swatch grid gets the full panel height instead of competing with the
 * preset list.
 */
export default function LutPanel() {
  const adjustments = useEditorStore((s) => s.adjustments);
  const setEditor = useEditorStore((s) => s.setEditor);
  const { setAdjustments, handleLutSelect } = useEditorActions();

  const handleLutIntensityChange = useCallback(
    (intensity: number) => setAdjustments((prev: Partial<Adjustments>) => ({ ...prev, lutIntensity: intensity })),
    [setAdjustments],
  );

  const handleLutClear = useCallback(() => {
    setAdjustments((prev: Partial<Adjustments>) => ({
      ...prev,
      lutPath: null,
      lutName: null,
      lutData: null,
      lutSize: 0,
      lutIntensity: 100,
      lutIsSceneReferred: false,
    }));
  }, [setAdjustments]);

  const handleDragStateChange = useCallback(
    (isDragging: boolean) => setEditor({ isSliderDragging: isDragging }),
    [setEditor],
  );

  return (
    <div className="flex flex-col h-full">
      <div className="p-3 flex justify-between items-center shrink-0 border-b border-surface">
        <div className="flex items-center gap-2">
          <LutIcon size={18} className="text-text-secondary" />
          <Text variant={TextVariants.title}>LUT</Text>
        </div>
      </div>
      <div className="grow overflow-y-auto custom-scrollbar p-3">
        <LUTControl
          lutPath={adjustments.lutPath || null}
          lutName={adjustments.lutName || null}
          lutIntensity={adjustments.lutIntensity ?? 100}
          onLutSelect={handleLutSelect}
          onIntensityChange={handleLutIntensityChange}
          onClear={handleLutClear}
          onDragStateChange={handleDragStateChange}
        />
      </div>
    </div>
  );
}
