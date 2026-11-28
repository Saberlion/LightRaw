import { useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { dirname, homeDir } from '@tauri-apps/api/path';
import { toast } from 'react-toastify';
import i18n from 'i18next';
import { useLibraryStore } from '../store/useLibraryStore';
import { canvasToolState, useEditorStore } from '../store/useEditorStore';
import { useUIStore } from '../store/useUIStore';
import { useProcessStore } from '../store/useProcessStore';
import { useSettingsStore } from '../store/useSettingsStore';
import { Invokes } from '../types/invokes';
import { LibraryViewMode } from '../types/settings';
import { ImageFile } from '../types/library';
import { INITIAL_ADJUSTMENTS } from '../utils/adjustments';
import { adjustmentsFromMetadata } from '../utils/developDefaults';
import { isSameValue } from '../utils/editHistory';
import { globalImageCache } from '../utils/ImageLRUCache';
import { decodeImage } from '../utils/decodeImage';
import { debouncedSave, debouncedSetHistory } from './useEditorActions';
import { notifyEmbeddedPreview } from './useImageLoader';
import { clearLibrarySelection } from './useLibraryActions';
import { computeSortedLibrary } from './useSortedLibrary';
import { getImmichAlbumImages, isImmichAlbumId } from '../utils/immichApi';

export interface AppNavigationProps {
  clearThumbnailQueue: () => void;
  // Late-bound import entry point (handleImportClick from useFileOperations).
  // useAppNavigation runs before useFileOperations in App.tsx while the latter
  // depends on this hook's handleSelectSubfolder - a cycle - so App.tsx stores
  // the importer in this ref right after useFileOperations returns.
  importFilesRef?: React.MutableRefObject<
    ((targetPath: string, source?: 'files' | 'album') => Promise<void>) | null
  >;
  refs: {
    transformWrapperRef: React.RefObject<any>;
    preloadedDataRef: React.RefObject<any>;
    cachedEditStateRef: React.RefObject<any>;
    selectedImagePathRef: React.RefObject<string | null>;
    isBackendReadyRef: React.RefObject<boolean>;
    latestRenderedJobIdRef: React.RefObject<number>;
    previewJobIdRef: React.RefObject<number>;
    currentResRef: React.RefObject<number>;
    prevAdjustmentsRef: React.RefObject<any>;
  };
}

let exifLoadGeneration = 0;
let imageSelectGeneration = 0;

const EXIF_CHUNK_SIZE = 250;

const loadExifForImages = async (
  files: ImageFile[],
  expectedFolderPath: string | null,
  sortKey: string,
  setLibrary: (updater: any) => void,
) => {
  const exifSortKeys = ['date_taken', 'iso', 'shutter_speed', 'aperture', 'focal_length'];
  const isExifSortActive = exifSortKeys.includes(sortKey);
  const generation = ++exifLoadGeneration;

  const previousExif = useLibraryStore.getState().exifByPath;
  const knownExif: Record<string, Record<string, string>> = {};
  for (const file of files) {
    const exif = previousExif[file.path] || file.exif;
    if (exif) knownExif[file.path] = exif;
  }

  if (files.length === 0) {
    setLibrary({ imageList: files, exifByPath: knownExif });
    return;
  }

  const paths = files.map((f: ImageFile) => f.path);

  if (isExifSortActive) {
    for (let i = 0; i < paths.length; i += EXIF_CHUNK_SIZE) {
      const chunk = paths.slice(i, i + EXIF_CHUNK_SIZE);
      try {
        const chunkExif: Record<string, Record<string, string>> = await invoke(Invokes.ReadExifForPaths, {
          paths: chunk,
        });
        Object.assign(knownExif, chunkExif);
      } catch (err) {
        console.error('Failed to read EXIF chunk:', err);
      }
    }

    if (generation !== exifLoadGeneration) return;
    setLibrary({ imageList: files, exifByPath: knownExif });
  } else {
    setLibrary({ imageList: files, exifByPath: knownExif });

    setTimeout(() => {
      const fetchExifInChunks = async () => {
        for (let i = 0; i < paths.length; i += EXIF_CHUNK_SIZE) {
          if (generation !== exifLoadGeneration) break;
          if (useLibraryStore.getState().currentFolderPath !== expectedFolderPath) break;

          const chunk = paths.slice(i, i + EXIF_CHUNK_SIZE);
          try {
            const chunkExif: Record<string, Record<string, string>> = await invoke(Invokes.ReadExifForPaths, {
              paths: chunk,
            });
            if (generation !== exifLoadGeneration) break;
            setLibrary((state: any) => ({ exifByPath: { ...state.exifByPath, ...chunkExif } }));
            await new Promise((resolve) => setTimeout(resolve, 50));
          } catch (err) {
            console.error('Failed to read EXIF chunk:', err);
          }
        }
      };
      fetchExifInChunks();
    }, 500);
  }
};

export function useAppNavigation({ clearThumbnailQueue, importFilesRef, refs }: AppNavigationProps) {
  const {
    transformWrapperRef,
    preloadedDataRef,
    cachedEditStateRef,
    selectedImagePathRef,
    isBackendReadyRef,
    latestRenderedJobIdRef,
    previewJobIdRef,
    currentResRef,
    prevAdjustmentsRef,
  } = refs;

  const handleGoHome = useCallback(() => {
    useLibraryStore.getState().setLibrary({
      rootPaths: [],
      currentFolderPath: null,
      activeAlbumId: null,
      imageList: [],
      imageRatings: {},
      folderTrees: [],
      multiSelectedPaths: [],
      libraryActivePath: null,
      expandedFolders: new Set(),
    });
    useUIStore.getState().setUI({ isLibraryExportPanelVisible: false });
  }, []);

  const handleBackToLibrary = useCallback(() => {
    const { selectedImage } = useEditorStore.getState();
    const { setLibrary } = useLibraryStore.getState();
    const { setUI } = useUIStore.getState();

    if (selectedImage?.path && cachedEditStateRef.current) {
      globalImageCache.set(selectedImage.path, cachedEditStateRef.current);
    }
    if (transformWrapperRef.current) {
      transformWrapperRef.current.resetTransform(0);
    }
    useEditorStore.getState().setEditor({
      zoom: 1,
      showOriginal: false,
      previewOverride: null,
    });

    debouncedSave.flush();
    debouncedSetHistory.flush();

    const lastActivePath = selectedImage?.path ?? null;
    const isStillVisible =
      lastActivePath !== null &&
      computeSortedLibrary(useLibraryStore.getState(), useSettingsStore.getState()).some(
        (img) => img.path === lastActivePath,
      );

    if (isStillVisible) {
      setLibrary({ libraryActivePath: lastActivePath });
    } else {
      clearLibrarySelection();
    }
    // isFullScreen must not survive an editor exit: it is only cleared inside
    // toggleFullScreen otherwise, and re-entering the editor would reopen into
    // the chrome-less preview (no toolbar/sheet, keyboard-only exits).
    setUI({ activeView: 'library', slideDirection: 1, isFullScreen: false });
  }, [refs]);

  const handleImageSelect = useCallback(
    async (path: string, openInEditor: boolean = true) => {
      const generation = ++imageSelectGeneration;
      const { selectedImage, isSliderDragging, resetHistory, stashHistory, setEditor } = useEditorStore.getState();
      const { setLibrary } = useLibraryStore.getState();
      const { setUI } = useUIStore.getState();
      const wasEditorVisible = useUIStore.getState().activeView === 'editor';

      if (openInEditor) {
        setUI({ activeView: 'editor' });
      }

      if (selectedImage?.path === path) return;

      useEditorStore.getState().patchesSentToBackend.clear();
      debouncedSave.flush();

      if (selectedImage?.path && cachedEditStateRef.current) {
        globalImageCache.set(selectedImage.path, cachedEditStateRef.current);
      }

      const cachedThumb = useProcessStore.getState().thumbnails[path];
      const cachedMedium = useProcessStore.getState().mediumThumbnails[path] || cachedThumb;

      const cached = globalImageCache.get(path);
      const isFrontendCached = Boolean(cached && cached.selectedImage?.isReady);
      const canKeepWgpuLive =
        useSettingsStore.getState().appSettings?.useWgpuRenderer !== false &&
        wasEditorVisible &&
        useEditorStore.getState().wgpuPhase === 'live';
      const isCachedInBackend = canKeepWgpuLive
        ? await invoke<boolean>('is_image_cached', { path }).catch(() => false)
        : false;

      if (generation !== imageSelectGeneration) return;

      const keepWgpuLive = canKeepWgpuLive && isCachedInBackend && useEditorStore.getState().wgpuPhase === 'live';

      if (!keepWgpuLive) {
        await decodeImage((isFrontendCached && cached?.finalPreviewUrl) || cachedMedium);
        if (generation !== imageSelectGeneration) return;
        setEditor({ wgpuPhase: 'hidden' });
      }

      stashHistory();
      selectedImagePathRef.current = path;

      const { multiSelectedPaths } = useLibraryStore.getState();
      const newMultiSelectedPaths = multiSelectedPaths.includes(path) ? multiSelectedPaths : [path];

      setLibrary({
        multiSelectedPaths: newMultiSelectedPaths,
        libraryActivePath: path,
        selectionAnchorPath: path,
      });

      setEditor({
        showOriginal: false,
        activeMaskId: null,
        activeMaskContainerId: null,
        activeAiPatchContainerId: null,
        activeAiSubMaskId: null,
        ...canvasToolState(),
        activeRelightLightId: null,
        previewOverride: null,
      });

      setUI({
        isLibraryExportPanelVisible: false,
        compactEditorPanelHeightOverride: null,
      });

      if (isFrontendCached && cached) {
        setEditor({
          selectedImage: {
            ...cached.selectedImage,
            thumbnailUrl: cachedMedium || cached.selectedImage.thumbnailUrl,
          },
          originalSize: cached.originalSize,
          histogram: cached.histogram,
          waveform: cached.waveform,
          finalPreviewUrl: cached.finalPreviewUrl,
          uncroppedAdjustedPreviewUrl: cached.uncroppedPreviewUrl,
        });

        setEditor({ adjustments: cached.adjustments });
        resetHistory(cached.adjustments, path);
        prevAdjustmentsRef.current = { path, adjustments: cached.adjustments };

        setLibrary({ isViewLoading: false });

        latestRenderedJobIdRef.current = previewJobIdRef.current;
        isBackendReadyRef.current = false;
        currentResRef.current = Infinity;

        invoke(Invokes.LoadImage, { path })
          .then((_result: any) => {
            if (selectedImagePathRef.current !== path) return;
            isBackendReadyRef.current = true;
            currentResRef.current = 0;
            setEditor({ originalSize: { width: _result.width, height: _result.height } });
            notifyEmbeddedPreview(_result);
          })
          .catch((err: any) => {
            if (String(err).includes('cancelled')) return;
            console.error('Background load_image failed on cache hit:', err);
            isBackendReadyRef.current = true;
            currentResRef.current = 0;
          });

        invoke(Invokes.LoadMetadata, { path })
          .then((metadata: any) => {
            if (selectedImagePathRef.current !== path) return;
            const freshAdjustments = adjustmentsFromMetadata(metadata.adjustments);
            if (!isSliderDragging && !isSameValue(cached.adjustments, freshAdjustments)) {
              setEditor({ adjustments: freshAdjustments });
              resetHistory(freshAdjustments, path);
              prevAdjustmentsRef.current = { path, adjustments: freshAdjustments };
              globalImageCache.set(path, { ...cached, adjustments: freshAdjustments });
            }
          })
          .catch((err) => console.error('Failed background metadata sync on cache hit:', err));

        return;
      }

      isBackendReadyRef.current = true;

      const imageFile = useLibraryStore.getState().imageList.find((img) => img.path === path);
      setEditor({
        selectedImage: {
          exif: null,
          group_id: imageFile?.group_id ?? null,
          height: 0,
          isRaw: false,
          isReady: false,
          metadata: null,
          path,
          thumbnailUrl: cachedMedium,
          width: 0,
        },
        originalSize: { width: 0, height: 0 },
        histogram: null,
        waveform: null,
        uncroppedAdjustedPreviewUrl: null,
      });

      setLibrary({ isViewLoading: true });

      setEditor((state) => {
        const prev = state.finalPreviewUrl;
        if (prev?.startsWith('blob:') && !globalImageCache.isProtected(prev)) {
          setTimeout(() => {
            if (!globalImageCache.isProtected(prev)) {
              URL.revokeObjectURL(prev);
            }
          }, 250);
        }
        return { finalPreviewUrl: null };
      });

      setEditor((state) => {
        if (state.interactivePatch?.url) URL.revokeObjectURL(state.interactivePatch.url);
        return { interactivePatch: null };
      });
    },
    [refs],
  );

  const handleSelectSubfolder = useCallback(
    async (
      path: string | null,
      isNewRoot = false,
      preloadedImages?: ImageFile[],
      expandParents = true,
      preserveEditor = false,
      skipHistory = false,
    ) => {
      const { appSettings, handleSettingsChange } = useSettingsStore.getState();
      const { pinnedFolders } = appSettings || { pinnedFolders: [] };
      const { setLibrary, sortCriteria } = useLibraryStore.getState();
      const { setUI } = useUIStore.getState();
      const { setProcess } = useProcessStore.getState();
      const { selectedImage, resetHistory, stashHistory, setEditor } = useEditorStore.getState();
      const libraryViewMode = appSettings?.libraryViewMode;

      if (!skipHistory && path) {
        useLibraryStore.getState().pushNavHistory({ type: 'folder', path });
      }

      if (!preserveEditor) {
        await invoke('cancel_thumbnail_generation');
        clearThumbnailQueue();
        setLibrary({ isViewLoading: true, activeAlbumId: null, libraryScrollTop: 0 });
        setProcess({ thumbnails: {}, mediumThumbnails: {} });
        globalImageCache.clear();
        setUI({ activeView: 'library', isFullScreen: false });
      } else {
        setLibrary({ isViewLoading: true, activeAlbumId: null });
      }

      try {
        const { rootPaths, expandedFolders: currentExpandedFolders } = useLibraryStore.getState();
        let newExpandedFolders = new Set(currentExpandedFolders);

        if (isNewRoot && path) {
          newExpandedFolders = new Set([path]);
          if (appSettings) {
            handleSettingsChange({ ...appSettings, lastRootPath: path } as any);
          }
        } else if (path && expandParents) {
          const allRoots = [...(rootPaths || []), ...(pinnedFolders || [])].filter(Boolean) as string[];
          const isInsideRoot = (r: string) =>
            path === r || (path.startsWith(r) && (/[\\/]$/.test(r) || ['/', '\\'].includes(path[r.length])));
          const relevantRoot = allRoots
            .filter(isInsideRoot)
            .reduce<string | undefined>((best, r) => (!best || r.length > best.length ? r : best), undefined);

          if (relevantRoot) {
            const separator = path.includes('/') ? '/' : '\\';
            const parentSeparatorIndex = path.lastIndexOf(separator);

            if (parentSeparatorIndex > -1 && path.length > relevantRoot.length) {
              let current = path.substring(0, parentSeparatorIndex);
              while (current && current.length >= relevantRoot.length) {
                newExpandedFolders.add(current);
                const nextParentIndex = current.lastIndexOf(separator);
                if (nextParentIndex === -1 || current === relevantRoot) break;
                current = current.substring(0, nextParentIndex);
              }
            }
            newExpandedFolders.add(relevantRoot);
          }
        }

        setLibrary({
          currentFolderPath: path,
          expandedFolders: newExpandedFolders,
          ...(preserveEditor ? {} : { imageList: [], multiSelectedPaths: [], libraryActivePath: null }),
        });

        if (!preserveEditor && selectedImage) {
          debouncedSave.flush();
          stashHistory();
          setEditor({ selectedImage: null, finalPreviewUrl: null, uncroppedAdjustedPreviewUrl: null, histogram: null });
          setEditor({ adjustments: INITIAL_ADJUSTMENTS });
          resetHistory(INITIAL_ADJUSTMENTS, null);
          useEditorStore.getState().patchesSentToBackend.clear();
        }

        const command =
          libraryViewMode === LibraryViewMode.Recursive ? Invokes.ListImagesRecursive : Invokes.ListImagesInDir;

        let files: ImageFile[];
        if (preloadedImages) {
          files = preloadedImages;
        } else {
          files = await invoke(command, { path });
          if (useLibraryStore.getState().currentFolderPath !== path) return;
        }

        const initialRatings: Record<string, number> = {};
        files.forEach((f) => {
          if (f.rating !== undefined) {
            initialRatings[f.path] = f.rating;
          }
        });
        setLibrary({ imageRatings: initialRatings });

        await loadExifForImages(files, path, sortCriteria.key, setLibrary);

        if (!preserveEditor) {
          invoke(Invokes.StartBackgroundIndexing, { folderPath: path }).catch((err) => {
            console.error('Failed to start background indexing:', err);
          });
        }
      } catch (err) {
        console.error('Failed to load folder contents:', err);
        toast.error('Failed to load images from the selected folder.');
      } finally {
        if (useLibraryStore.getState().currentFolderPath === path) {
          useLibraryStore.getState().setLibrary({ isViewLoading: false });
        }
      }
    },
    [clearThumbnailQueue, refs],
  );

  const handleSelectAlbum = useCallback(
    async (albumId: string, albumName: string, imagePaths: string[], preserveEditor = false, skipHistory = false) => {
      const { setLibrary, sortCriteria } = useLibraryStore.getState();
      const { setUI } = useUIStore.getState();

      if (!skipHistory) {
        useLibraryStore.getState().pushNavHistory({ type: 'album', path: albumId, albumName, images: imagePaths });
      }

      if (!preserveEditor) {
        await invoke('cancel_thumbnail_generation');
        clearThumbnailQueue();
        setLibrary({ libraryScrollTop: 0 });
        globalImageCache.clear();
        setUI({ activeView: 'library', isFullScreen: false });
      }

      const albumFolderPath = `Album: ${albumName}`;
      const isCurrentAlbum = () => {
        const { currentFolderPath, activeAlbumId } = useLibraryStore.getState();
        return currentFolderPath === albumFolderPath && activeAlbumId === albumId;
      };

      setLibrary({
        isViewLoading: true,
        currentFolderPath: albumFolderPath,
        activeAlbumId: albumId,
      });

      try {
        const files: ImageFile[] = isImmichAlbumId(albumId)
          ? await getImmichAlbumImages(albumId)
          : await invoke(Invokes.GetAlbumImages, { paths: imagePaths });
        if (!isCurrentAlbum()) return true;

        const initialRatings: Record<string, number> = {};
        files.forEach((f) => {
          if (f.rating !== undefined) initialRatings[f.path] = f.rating;
        });

        setLibrary({
          imageRatings: initialRatings,
          ...(preserveEditor ? {} : { multiSelectedPaths: [], libraryActivePath: null }),
        });

        await loadExifForImages(files, albumFolderPath, sortCriteria.key, setLibrary);
        return true;
      } catch (err) {
        console.error('Failed to load album images:', err);
        toast.error(`Failed to load album: ${err}`);
        return false;
      } finally {
        if (isCurrentAlbum()) {
          setLibrary({ isViewLoading: false });
        }
      }
    },
    [clearThumbnailQueue],
  );

  const handleNavBack = useCallback(async () => {
    const { navHistory, navIndex, setLibrary } = useLibraryStore.getState();

    if (navIndex > 0) {
      const targetIndex = navIndex - 1;
      const target = navHistory[targetIndex];
      setLibrary({ navIndex: targetIndex });

      if (target.type === 'folder') {
        handleSelectSubfolder(target.path, false, undefined, true, false, true);
      } else if (target.type === 'album') {
        handleSelectAlbum(target.path, target.albumName!, target.images!, false, true);
      }
    }
  }, [handleSelectSubfolder, handleSelectAlbum]);

  const handleNavForward = useCallback(async () => {
    const { navHistory, navIndex, setLibrary } = useLibraryStore.getState();

    if (navIndex < navHistory.length - 1) {
      const targetIndex = navIndex + 1;
      const target = navHistory[targetIndex];
      setLibrary({ navIndex: targetIndex });

      if (target.type === 'folder') {
        handleSelectSubfolder(target.path, false, undefined, true, false, true);
      } else if (target.type === 'album') {
        handleSelectAlbum(target.path, target.albumName!, target.images!, false, true);
      }
    }
  }, [handleSelectSubfolder, handleSelectAlbum]);

  const handleOpenContainingFolder = useCallback(
    async (filePath: string) => {
      const folderPath = await dirname(filePath);
      if (!folderPath || useLibraryStore.getState().currentFolderPath === folderPath) return;
      await handleSelectSubfolder(folderPath, false, undefined, true, true);
    },
    [handleSelectSubfolder],
  );

  // intent: 'open' = the welcome splash's primary entry (first run / "open
  // library"), 'add' = explicit add-folder. Phones resolve 'open' to the
  // internal library root; 'add' always opens the system folder picker.
  const handleOpenFolder = useCallback(
    async (intent: 'open' | 'add' = 'add') => {
      const { osPlatform, isOhos, appSettings, handleSettingsChange } = useSettingsStore.getState();
      const { rootPaths, folderTrees, setLibrary } = useLibraryStore.getState();
      const isAndroid = osPlatform === 'android';

      try {
        let selectedPath = '';
        if (isAndroid) {
          selectedPath = await invoke<string>(Invokes.GetOrCreateInternalLibraryRoot);
        } else if (isOhos) {
          // OHOS: tauri-plugin-dialog is excluded (its rfd backend has no
          // OpenHarmony support); the ArkTS DocumentViewPicker bridge returns
          // the picked folder path (null when the user cancelled).
          // intent distinguishes the two call sites: 'open' is the welcome
          // splash's primary entry, while 'add' is any splash tap when a
          // library already exists. On a phone neither may touch the
          // directory picker: folder grants cannot persist there and the
          // library is the internal root, so 'open' enters it directly and
          // 'add' opens the FILE picker instead - the picked images import
          // straight into the internal root (the FileMgr's folder-mode UI
          // is a phone-skinned directory tree that does not fit this flow).
          // Phones AND tablets (2026-11-28, product decision) open the
          // internal library root and, on 'add', the FILE picker that
          // imports straight into it. The folder picker stays 2in1-only:
          // the directory-tree UI fits the desktop form factor, and 2in1
          // grants persist. (Tablet grants DO persist - TC-FILE-09 - but
          // the internal-root flow keeps the two touch form factors
          // consistent.)
          const deviceType = useSettingsStore.getState().ohosDeviceType;
          if (deviceType === 'phone' || deviceType === 'tablet') {
            const internalRoot = await invoke<string>(Invokes.GetOrCreateInternalLibraryRoot);
            if (intent === 'add' && importFilesRef?.current) {
              await importFilesRef.current(internalRoot, 'files');
            }
            selectedPath = internalRoot;
          } else {
            selectedPath = ((await invoke<string | null>(Invokes.PickOhosFolder)) ?? '');
          }
        } else {
          const selected = await open({ directory: true, multiple: false, defaultPath: await homeDir() });
          if (typeof selected === 'string') {
            selectedPath = selected;
          }
        }

        if (selectedPath) {
          if (!rootPaths.includes(selectedPath)) {
            const newRootPaths = [...rootPaths, selectedPath];
            setLibrary({ rootPaths: newRootPaths });

            if (appSettings) {
              handleSettingsChange({ ...appSettings, rootFolders: newRootPaths } as any);
            }

            setLibrary({ isTreeLoading: true });
            try {
              const newTree = await invoke(Invokes.GetFolderTree, {
                path: selectedPath,
                expandedFolders: [selectedPath],
                showImageCounts:
                  appSettings?.enableFolderImageCounts || appSettings?.folderTreeSort?.key === 'imageCount',
              });
              setLibrary({ folderTrees: [...folderTrees, newTree] });
            } catch (e) {
              toast.error(`Failed to load folder tree: ${e}`);
            } finally {
              setLibrary({ isTreeLoading: false });
            }
          }
          await handleSelectSubfolder(selectedPath, true);
        }
      } catch (err) {
        console.error(isAndroid ? 'Failed to open Android library root:' : 'Failed to open directory dialog:', err);
        toast.error(isAndroid ? 'Failed to open library.' : 'Failed to open folder selection dialog.');
      }
    },
    [handleSelectSubfolder, importFilesRef],
  );

  const handleContinueSession = () => {
    const restore = async () => {
      const { appSettings, isOhos, handleSettingsChange } = useSettingsStore.getState();
      const { setLibrary } = useLibraryStore.getState();

      let rootFolders = appSettings?.rootFolders?.length
        ? appSettings.rootFolders
        : appSettings?.lastRootPath
          ? [appSettings.lastRootPath]
          : [];

      if (rootFolders.length === 0) return;

      const folderState = appSettings?.lastFolderState;
      let pathToSelect = folderState?.currentFolderPath || rootFolders[0];

      // OpenHarmony: FileKit picker grants do not survive app reinstalls, so
      // saved roots turn unreadable until the user re-picks them — and the
      // tree/children scanners swallow the read_dir EPERM, which would
      // restore into an empty library (porting doc 6.13). Probe the roots
      // first and re-request authorization for the dead ones.
      if (isOhos) {
        let aliveRoots: string[] | null = null;
        try {
          const readable = await invoke<boolean[]>(Invokes.CheckPathsReadable, { paths: rootFolders });
          aliveRoots = rootFolders.filter((_, i) => readable[i] === true);
        } catch (err) {
          console.warn('Failed to probe library root readability, skipping re-grant flow', err);
        }

        if (aliveRoots !== null && aliveRoots.length < rootFolders.length) {
          const alive = aliveRoots;
          console.warn(
            'Library roots unreadable (FileKit grant revoked?), requesting re-grant:',
            rootFolders.filter((r) => !alive.includes(r)),
          );
          toast.info(i18n.t('library.toasts.regrantNeeded'));
          const repicked = await invoke<string | null>(Invokes.PickOhosFolder);

          if (repicked) {
            const underRoot = (p: string, root: string) =>
              p === root || p.startsWith(root.endsWith('/') ? root : root + '/');
            rootFolders = alive.filter((r) => !underRoot(r, repicked) && !underRoot(repicked, r));
            rootFolders.push(repicked);
            if (!pathToSelect.startsWith('Album: ') && !rootFolders.some((r) => underRoot(pathToSelect, r))) {
              pathToSelect = repicked;
            }
            // The startup preload resolved against dead roots (empty trees);
            // drop it so the restore below loads fresh data.
            preloadedDataRef.current = undefined;
            if (appSettings) {
              await handleSettingsChange({ ...appSettings, rootFolders });
            }
          } else if (alive.length === 0) {
            toast.error(i18n.t('library.toasts.restoreFailed'));
            // The startup preload was resolved while the roots were dead
            // (empty trees). If the roots become readable again later and
            // the user retries "continue session", the probe now passes and
            // the restore below would otherwise reuse that stale empty
            // preload -> a 0/0 library until an app restart (BUG-2026-11-28-16).
            preloadedDataRef.current = undefined;
            handleGoHome();
            return;
          } else {
            rootFolders = alive;
          }
        }
      }

      // FUSE user-storage roots (Download etc., via FileKit grants) are
      // cloud-synced mounts: "optimized storage" offloads surface as 0-byte
      // stubs that render as permanent grey placeholder cards, and writes
      // depend on a grant that must be re-activated on every launch (the
      // entire failure chain of porting doc §6.40). The app-internal library
      // root is the recommended default. Warn once — not on every launch.
      const isFuseUserStorage = (p: string) =>
        p.startsWith('/storage/Users/') || p.startsWith('/storage/media/');
      if (
        rootFolders.some(isFuseUserStorage) &&
        !localStorage.getItem('lightraw.fuse-root-notice')
      ) {
        localStorage.setItem('lightraw.fuse-root-notice', '1');
        toast.info(i18n.t('library.toasts.fuseRootNotice'), { autoClose: 10000 });
      }

      setLibrary({ rootPaths: rootFolders });

      if (folderState?.expandedFolders) {
        const newExpandedFolders = new Set<string>(folderState.expandedFolders);
        setLibrary({ expandedFolders: newExpandedFolders });
      } else {
        setLibrary({ expandedFolders: new Set(rootFolders) });
      }

      setLibrary({ isTreeLoading: true });
      try {
        let treesData;
        if (preloadedDataRef.current?.rootPaths?.join() === rootFolders.join() && preloadedDataRef.current.trees) {
          treesData = await preloadedDataRef.current.trees;
          preloadedDataRef.current.trees = undefined;
        } else {
          const expandedArr = folderState?.expandedFolders
            ? Array.from(new Set(folderState.expandedFolders))
            : rootFolders;
          treesData = await invoke(Invokes.GetPinnedFolderTrees, {
            paths: rootFolders,
            expandedFolders: expandedArr,
            showImageCounts: appSettings?.enableFolderImageCounts || appSettings?.folderTreeSort?.key === 'imageCount',
          });
        }
        setLibrary({ folderTrees: treesData });
      } catch (err) {
        console.error('Failed to restore folder trees:', err);
      } finally {
        setLibrary({ isTreeLoading: false });
      }

      let preloadedImages: ImageFile[] | undefined = undefined;
      if (preloadedDataRef.current?.currentPath === pathToSelect && preloadedDataRef.current.images) {
        try {
          preloadedImages = await preloadedDataRef.current.images;
          preloadedDataRef.current.images = undefined;
        } catch (e) {
          console.error('Failed to retrieve preloaded images', e);
        }
      }

      if (pathToSelect && pathToSelect.startsWith('Album: ')) {
        const activeAlbumId = folderState?.activeAlbumId;
        if (activeAlbumId) {
          try {
            const albumTree: any = await invoke(Invokes.GetAlbums);
            setLibrary({ albumTree });

            const findObj = (nodes: any[]): any => {
              for (const n of nodes) {
                if (n.id === activeAlbumId) return n;
                if (n.type === 'group') {
                  const f = findObj(n.children);
                  if (f) return f;
                }
              }
              return null;
            };

            const album = findObj(albumTree);
            if (album) {
              await handleSelectAlbum(album.id, album.name, album.images);
            } else if (
              !isImmichAlbumId(activeAlbumId) ||
              !(await handleSelectAlbum(activeAlbumId, pathToSelect.slice('Album: '.length), []))
            ) {
              await handleSelectSubfolder(rootFolders[0], false, undefined, false);
            }
          } catch (e) {
            console.error('Failed to restore album session:', e);
            await handleSelectSubfolder(rootFolders[0], false, undefined, false);
          }
        } else {
          await handleSelectSubfolder(rootFolders[0], false, undefined, false);
        }
      } else {
        await handleSelectSubfolder(pathToSelect, false, preloadedImages, false);
      }
    };

    restore().catch((err) => {
      console.error('Failed to restore session:', err);
      toast.error(i18n.t('library.toasts.restoreFailed'));
      handleGoHome();
      useLibraryStore.getState().setLibrary({ isTreeLoading: false });
    });
  };

  return {
    handleGoHome,
    handleBackToLibrary,
    handleImageSelect,
    handleSelectSubfolder,
    handleSelectAlbum,
    handleOpenFolder,
    handleOpenContainingFolder,
    handleNavBack,
    handleNavForward,
    handleContinueSession,
  };
}
