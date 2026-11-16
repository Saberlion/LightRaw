import { useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import clsx from 'clsx';
import {
  Images,
  FolderHeart,
  SwatchBook,
  Settings,
  Plus,
  Trash2,
  Copy,
  ClipboardPaste,
  FileInput,
  X,
} from 'lucide-react';

import { useUIStore } from '../store/useUIStore';
import { useLibraryStore } from '../store/useLibraryStore';
import { useEditorStore } from '../store/useEditorStore';
import { AlbumItem, Album } from '../types/library';
import { PhoneTab } from '../types/ui';
import PresetsPanel from './panel/right/PresetsPanel';

interface PhoneTabButtonProps {
  Icon: React.ComponentType<{ size?: number; className?: string }>;
  isActive: boolean;
  label: string;
  onClick(): void;
}

const PhoneTabButton = ({ Icon, isActive, label, onClick }: PhoneTabButtonProps) => (
  <button
    className={clsx(
      // 56px tall touch target on a 44px minimum budget; the pill bar is the
      // only fixed navigation on the phone layout, so targets stay generous.
      'flex-1 min-w-0 h-14 flex flex-col items-center justify-center gap-1 rounded-full transition-colors duration-150 select-none',
      isActive ? 'text-accent' : 'text-text-secondary active:text-text-primary',
    )}
    aria-current={isActive ? 'page' : undefined}
    onClick={onClick}
  >
    <Icon size={22} className="shrink-0" />
    <span className="text-[11px] leading-none font-medium max-w-full truncate px-1">{label}</span>
  </button>
);

interface PhoneBatchButtonProps {
  Icon: React.ComponentType<{ size?: number; className?: string }>;
  label: string;
  onClick(): void;
  disabled?: boolean;
  destructive?: boolean;
}

// Same icon-over-label anatomy as PhoneTabButton so the batch bar reads as the
// same container with different content (CamRAW pattern: the tab bar itself
// is replaced by the batch actions during multi-select).
const PhoneBatchButton = ({ Icon, label, onClick, disabled = false, destructive = false }: PhoneBatchButtonProps) => (
  <button
    className={clsx(
      'flex-1 min-w-0 h-14 flex flex-col items-center justify-center gap-1 rounded-full transition-colors duration-150 select-none',
      disabled
        ? 'text-text-secondary opacity-40 cursor-not-allowed'
        : destructive
          ? 'text-red-400 active:text-red-500'
          : 'text-text-secondary active:text-text-primary',
    )}
    disabled={disabled}
    onClick={() => !disabled && onClick()}
  >
    <Icon size={22} className="shrink-0" />
    <span className="text-[11px] leading-none font-medium max-w-full truncate px-1">{label}</span>
  </button>
);

interface PhoneAlbumsTabProps {
  onSelectAlbum(album: Album): void;
  onNewAlbum(): void;
}

function PhoneAlbumsTab({ onSelectAlbum, onNewAlbum }: PhoneAlbumsTabProps) {
  const { t } = useTranslation();
  const albumTree = useLibraryStore((s) => s.albumTree);

  const renderItem = useCallback(
    (item: AlbumItem, depth: number): React.ReactNode =>
      item.type === 'album' ? (
        <button
          key={item.id}
          className={clsx(
            'w-full min-h-14 flex items-center gap-3 px-4 rounded-xl bg-surface text-left',
            'active:bg-card-active transition-colors select-none',
          )}
          style={depth > 0 ? { paddingLeft: 16 + depth * 16 } : undefined}
          onClick={() => onSelectAlbum(item)}
        >
          <FolderHeart size={20} className="text-text-secondary shrink-0" />
          <span className="flex-1 min-w-0 truncate text-text-primary font-medium">{item.name}</span>
          <span className="text-text-secondary text-sm shrink-0">{item.images.length}</span>
        </button>
      ) : (
        <div key={item.id} className="flex flex-col gap-1">
          <div
            className="text-text-secondary text-xs font-semibold uppercase tracking-wide px-4 pt-3"
            style={depth > 0 ? { paddingLeft: 16 + depth * 16 } : undefined}
          >
            {item.name}
          </div>
          <div className="flex flex-col gap-1.5 px-2">{item.children.map((child) => renderItem(child, depth + 1))}</div>
        </div>
      ),
    [onSelectAlbum],
  );

  return (
    <div className="flex-1 min-h-0 flex flex-col gap-3 p-3">
      <div className="flex items-center justify-between px-1">
        <h1 className="text-2xl font-bold text-text-primary truncate">{t('library.folders.sections.albums')}</h1>
        <button
          className={clsx(
            'h-12 w-12 rounded-full bg-surface text-text-primary flex items-center justify-center shrink-0',
            'active:bg-card-active transition-colors select-none',
          )}
          data-tooltip={t('contextMenus.albums.newAlbum')}
          onClick={onNewAlbum}
        >
          <Plus size={22} />
        </button>
      </div>
      {albumTree.length === 0 ? (
        <div className="flex-1 flex flex-col items-center justify-center gap-4 pb-16">
          <FolderHeart size={40} className="text-text-secondary opacity-40" />
          <button
            className="h-12 px-6 rounded-full bg-accent text-button-text font-medium active:opacity-80 transition-opacity"
            onClick={onNewAlbum}
          >
            {t('contextMenus.albums.newAlbum')}
          </button>
        </div>
      ) : (
        <div className="flex-1 min-h-0 overflow-y-auto custom-scrollbar flex flex-col gap-1.5 pb-4">
          {albumTree.map((item) => renderItem(item, 0))}
        </div>
      )}
    </div>
  );
}

interface PhoneTabShellProps {
  /** Gallery tab content — the library grid (LibraryView). */
  children: React.ReactNode;
  /** Opens the tapped album and switches back to the gallery tab. */
  onSelectAlbum(album: Album): void;
  /** Opens the create-album modal (reuses the desktop flow). */
  onNewAlbum(): void;
  /** Renders the "me" tab content (SettingsPanel) — keeps App-level wiring here. */
  renderSettings(): React.ReactNode;
  /** Batch bar actions (gallery tab with an active multi-selection). */
  onBatchDelete(): void;
  onBatchExport(): void;
  onBatchCopy(): void;
  onBatchPaste(): void;
  /** Exits the selection (same chain as the library header cancel button). */
  onClearSelection(): void;
}

/**
 * Bottom tab shell for the OHOS phone (compact) library layout —
 * docs/HARMONYOS_PHONE_LAYOUT.md §3. Four tabs in a floating pill bar
 * (CamRAW pattern: icon + label, selected = accent color only, the bar never
 * grows a background block or indicator strip):
 *
 * - Gallery  — the existing LibraryView (grid + selection + import)
 * - Albums   — user albums from the library store, tap opens one
 * - Presets  — the presets panel as a full-width browse/manage page
 * - Settings — app settings
 *
 * The shell only switches views; the library data flow underneath keeps its
 * state because every tab keeps the gallery mounted (`hidden`), so switching
 * back never reloads the grid.
 */
export default function PhoneTabShell({
  children,
  onSelectAlbum,
  onNewAlbum,
  renderSettings,
  onBatchDelete,
  onBatchExport,
  onBatchCopy,
  onBatchPaste,
  onClearSelection,
}: PhoneTabShellProps) {
  const { t } = useTranslation();
  const phoneTab = useUIStore((s) => s.phoneTab);
  const setUI = useUIStore((s) => s.setUI);
  const multiSelectedPaths = useLibraryStore((s) => s.multiSelectedPaths);
  const totalImages = useLibraryStore((s) => s.imageList.length);
  // Reactive so the paste action enables as soon as adjustments are copied.
  const hasCopiedAdjustments = useEditorStore((s) => s.copiedAdjustments !== null);

  const setTab = (tab: PhoneTab) => setUI({ phoneTab: tab });

  const tabs: Array<{ id: PhoneTab; label: string; Icon: React.ComponentType<{ size?: number; className?: string }> }> =
    [
      { id: 'gallery', label: t('library.header.title'), Icon: Images },
      { id: 'albums', label: t('library.folders.sections.albums'), Icon: FolderHeart },
      { id: 'presets', label: t('editor.presets.title'), Icon: SwatchBook },
      { id: 'me', label: t('settings.title'), Icon: Settings },
    ];

  // CamRAW pattern: while images are selected on the gallery tab, the very
  // same pill container becomes the batch action bar — navigation stays one
  // "cancel" tap away and never competes with the actions for space.
  const isBatchMode = phoneTab === 'gallery' && multiSelectedPaths.length > 0;

  return (
    <div className="relative flex flex-col flex-1 min-h-0 h-full">
      {/* Keep every tab mounted; hide the inactive ones so the library grid
          (and its thumbnail pipeline) never remounts on tab switches.
          All tab content runs the FULL height: the pill nav floats over it
          (absolute, below), so the gallery grid scrolls edge-to-edge behind
          the nav — the grid only adds a spacer row (gridBottomPadding) so the
          last thumbnails can be scrolled up above the pill. */}
      <div className={clsx('flex-1 min-h-0', phoneTab === 'gallery' ? 'flex flex-col' : 'hidden')}>{children}</div>
      {phoneTab === 'albums' && (
        <div className="flex-1 min-h-0 flex flex-col pb-20">
          <PhoneAlbumsTab onSelectAlbum={onSelectAlbum} onNewAlbum={onNewAlbum} />
        </div>
      )}
      {phoneTab === 'presets' && (
        <div className="flex-1 min-h-0 flex flex-col overflow-y-auto custom-scrollbar pb-20">
          <PresetsPanel />
        </div>
      )}
      {phoneTab === 'me' && <div className="flex-1 min-h-0 flex flex-col">{renderSettings()}</div>}

      <nav
        className={clsx(
          // Floating pill (user design: the nav hovers OVER the page, the
          // image area extends behind it to the full screen).
          'absolute bottom-0 inset-x-0 mx-2 rounded-full bg-surface border border-border-color/20',
          'shadow-[0_4px_20px_rgba(0,0,0,0.18)] flex items-stretch p-1 z-40 pointer-events-auto',
        )}
        aria-label={isBatchMode ? t('library.header.selectItems') : t('library.header.title')}
      >
        {isBatchMode ? (
          <>
            <button
              className="h-14 w-14 shrink-0 flex items-center justify-center rounded-full text-text-secondary active:text-text-primary transition-colors"
              data-tooltip={t('library.header.cancel')}
              onClick={onClearSelection}
            >
              <X size={22} />
            </button>
            <div className="flex-1 min-w-0 flex flex-col items-center justify-center px-1">
              <span className="text-[11px] leading-none font-semibold text-text-primary tabular-nums truncate">
                {multiSelectedPaths.length} / {totalImages}
              </span>
            </div>
            <PhoneBatchButton Icon={Trash2} label={t('settings.data.delete')} onClick={onBatchDelete} destructive />
            <PhoneBatchButton Icon={Copy} label={t('ui.bottomBar.tooltips.copySettings')} onClick={onBatchCopy} />
            <PhoneBatchButton
              Icon={ClipboardPaste}
              label={t('ui.bottomBar.tooltips.pasteSettings')}
              onClick={onBatchPaste}
              disabled={!hasCopiedAdjustments}
            />
            <PhoneBatchButton Icon={FileInput} label={t('ui.bottomBar.tooltips.export')} onClick={onBatchExport} />
          </>
        ) : (
          tabs.map(({ id, label, Icon }) => (
            <PhoneTabButton key={id} Icon={Icon} label={label} isActive={phoneTab === id} onClick={() => setTab(id)} />
          ))
        )}
      </nav>
    </div>
  );
}
