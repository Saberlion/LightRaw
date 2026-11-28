import { RustAbility } from '@ohos-rs/ability'
import Want from '@ohos.app.ability.Want'
import { AbilityConstant, Configuration, ConfigurationConstant, common, abilityAccessCtrl, Permissions } from '@kit.AbilityKit';
import { BusinessError, commonEventManager } from '@kit.BasicServicesKit';
import { KeyboardAvoidMode } from '@ohos.arkui.UIContext';
import window from '@ohos.window';
import picker from '@ohos.file.picker';
import fileUri from '@ohos.file.fileuri';
import fs from '@ohos.file.fs';
import { fileShare } from '@kit.CoreFileKit';
import deviceInfo from '@ohos.deviceInfo';
import { photoAccessHelper } from '@kit.MediaLibraryKit';
import { dataSharePredicates } from '@kit.ArkData';
import { privacyManager } from '@kit.AppGalleryKit';
import wantAgent, { WantAgent } from '@ohos.app.ability.wantAgent';
import usbManager from '@ohos.usbManager';
import { wifiManager } from '@kit.ConnectivityKit';
import { backgroundTaskManager } from '@kit.BackgroundTasksKit';

// Media Library Kit resource handler for the gallery-import fallback path
// (see EntryAbility.stageGalleryImages): MediaAssetManager.requestImageData
// hands the prepared image bytes to onDataPrepared. Official sample pattern —
// arrow property so `this` stays bound.
class GalleryAssetDataHandler implements photoAccessHelper.MediaAssetDataHandler<ArrayBuffer> {
  private callback: (data: ArrayBuffer | null) => void;

  constructor(callback: (data: ArrayBuffer | null) => void) {
    this.callback = callback;
  }

  onDataPrepared = (data: ArrayBuffer, map?: Map<string, string>): void => {
    // The system passes undefined when preparation fails; type the param as
    // declared but funnel the failure to the caller as null.
    if (data === undefined || data === null) {
      console.error('[RapidRAW] gallery asset request prepared no data');
      this.callback(null);
      return;
    }
    console.info(
      `[RapidRAW] gallery asset data prepared (quality: ${map?.get('quality') ?? 'unknown'}, ${data.byteLength} bytes)`,
    );
    this.callback(data);
  };
}

export default class EntryAbility extends RustAbility {
  // change to dynamic library name
  public moduleName: string = "lightraw"
  // Load our own pages/Index instead of the crate's MainPage: the custom
  // page adds the page-level onBackPress bridge (docs 6.5). The webview is
  // still created by DefaultXComponent inside that page.
  public defaultPage: boolean = false;
  public mode: 'xcomponent' | 'webview' = 'webview'
  private rapidrawModule: ESObject | null = null;
  // Path of a file manager "open with" / share request that arrived before
  // the native module finished loading (onCreate runs first); flushed once
  // registerRapidrawWindowController resolves the module (docs 6.22).
  private pendingOpenPath: string | null = null;
  // Main window handle, kept so the theme-aware base color can be re-applied
  // when the system color mode changes (docs 6.25).
  private mainWindow: window.Window | null = null;
  // PRIVACY_STATE_CHANGED subscription (AppGallery standardized privacy
  // hosting); kept so a future teardown path can unsubscribe.
  private privacySubscriber: commonEventManager.CommonEventSubscriber | null = null;
  // Pending gallery-import staging job — see pickGalleryImages/stageGalleryImages.
  // The read grant behind picker-returned media URIs is NOT usable from inside
  // the picker's own select() callback: official Media Library Kit guidance is
  // to save the URIs and open the files only after the callback returns, and
  // opening them in-callback fails with 13900002 (ENOENT) on current
  // HarmonyOS builds — which silently dropped every selected image
  // (Mate 80, 2026-11-25). The job fields hold the parked copy work.
  private galleryPickRequestId: number = -1;
  private galleryPickResolver: ESObject | null = null;
  private galleryPickUris: Array<string> = [];
  private galleryPickTargets: Array<string> = [];
  private galleryPickStaged: Array<string> = [];
  private galleryPickCopied: Array<boolean> = [];
  private galleryPickAttempt: number = 0;
  // User-grant permissions already attempted this session — requestUserPermissionsOnce
  // consults this so each permission prompts at most once per session (the
  // system additionally remembers refusals across sessions, §6.43).
  private requestedPermissions: Set<string> = new Set<string>();
  // Copy attempts per pick and the back-off ladder between them. The first
  // attempt runs ~200ms after the picker closes (grant registration is
  // asynchronous); retries cover slower devices. Total worst case ~7.7s, but
  // the common case succeeds on attempt 1.
  private static readonly GALLERY_PICK_MAX_ATTEMPTS: number = 5;
  private static readonly GALLERY_PICK_RETRY_DELAYS_MS: Array<number> = [200, 500, 1000, 2000, 4000];

  async onCreate(want: Want, launchParam: AbilityConstant.LaunchParam): Promise<void> {
    super.onCreate(want, launchParam);
    // A cold start from the file manager ("open with") or a share panel
    // carries the target in want.uri / the share stream (docs 6.22).
    this.captureOpenWant(want);
    // Form-factor evidence for the folder-grant strategy (docs 6.14): 2in1
    // keeps persistent folder grants; tablets/phones stay on per-launch
    // re-grant because the persistence capability is 2in1-only.
    console.info(`[RapidRAW] deviceType: ${deviceInfo.deviceType}`);
    // Drop partial album-import staging from a previous run (cache, safe to
    // clear before any import starts).
    this.cleanupImportStaging();
    // Persistent folder grants are NOT auto-activated on app start: replay
    // the recorded ones now, ahead of the webview's library preload (which
    // fires seconds later). Fire-and-forget on purpose — awaiting lifecycle
    // helpers here once caused the ability to terminate (docs 6.4).
    this.activatePersistedGrants();
  }

  // Launch type is singleton, so a second "open with" / share while the app
  // is already running arrives as a new want instead of a new ability. The
  // Rust side re-emits it as the desktop `open-with-file` event, which the
  // frontend's existing launch pipeline consumes (docs 6.22).
  onNewWant(want: Want, launchParam: AbilityConstant.LaunchParam): void {
    console.info('[RapidRAW] onNewWant: forwarding open-with request');
    this.captureOpenWant(want);
  }

  // Back must not destroy the ability: the webview owns the navigation chain
  // (close modal -> close panel -> editor->library -> library->home). Consume the
  // gesture here and forward it to the Rust bridge, which re-emits
  // `ohos-back-pressed` for useAndroidBackHandler (D-25).
  onBackPressed(): boolean {
    const notify: ESObject =
      this.rapidrawModule?.notifyOhosBackPressed ?? this.rapidrawModule?.default?.notifyOhosBackPressed;
    console.info(`[RapidRAW] ability onBackPressed fired, notify=` + typeof notify);
    if (typeof notify === 'function') {
      notify();
      return true;
    }
    return false;
  }

  async onWindowStageCreate(windowStage: window.WindowStage): Promise<void> {
    // NOTE: local name must NOT be `window` — it would shadow the module
    // import and break enum access like window.AvoidAreaType (docs 6.8).
    const win = windowStage.getMainWindowSync();
    await win.setWindowLayoutFullScreen(false);
    // Base color before the page loads: dark -> black, otherwise white. The
    // webview is transparent end to end, so this is what the first frame
    // shows instead of the window's black surface (docs 6.25).
    this.mainWindow = win;
    this.applyBaseColor(this.context.config?.colorMode);
    // Safe-area: with layoutFullScreen(false) the window manager reserves the
    // system bars already; the remaining avoid rect (cutout, gesture strip)
    // rides the native bridge so the webview can pad for it — ArkWeb's
    // env(safe-area-inset-*) proved unreliable (phone layout G1). The initial
    // push happens once the native module is registered below; this
    // subscription covers later changes (rotation, fold, keyboard).
    try {
      win.on('avoidAreaChange', () => {
        this.pushAvoidAreas(win);
      });
      console.info('[RapidRAW] avoidAreaChange subscribed');
    } catch (e) {
      console.error(`[RapidRAW] avoidAreaChange subscribe failed: ${JSON.stringify(e)}`);
    }
    // Idle auto-download (docs/AI_MODEL_AUTO_DOWNLOAD.md): focus loss on a
    // 2in1 multi-window desktop is a system-provided idle signal. ACTIVE =
    // focused (idle clock reset), INACTIVE = unfocused (clock starts).
    try {
      windowStage.on('windowStageEvent', (stageEvent: window.WindowStageEventType) => {
        const focused: boolean = stageEvent === window.WindowStageEventType.ACTIVE;
        const unfocused: boolean = stageEvent === window.WindowStageEventType.INACTIVE;
        if (focused || unfocused) {
          this.pushIdleEvent('window', focused);
        }
      });
      console.info('[RapidRAW] windowStageEvent subscribed');
    } catch (e) {
      console.error(`[RapidRAW] windowStageEvent subscribe failed: ${JSON.stringify(e)}`);
    }
    // RapidRAW's custom titlebar drives window controls through tauri
    // commands into ohos_integration.rs, which calls back into the JS
    // callback registered here (tao-ohos stubs the native window ops).
    // Fire-and-forget on purpose: awaiting lifecycle helpers here once
    // caused the ability to terminate right after startup (docs 6.4).
    this.registerRapidrawWindowController(win);
    super.onWindowStageCreate(windowStage);
    await windowStage.loadContent('pages/Index');
    // Window-level back interception (D-25): the AppGallery privacy
    // UIExtension otherwise consumes the gesture and disposes itself, which
    // takes the app window down with it. Register a window handler first so
    // the gesture is forwarded to the webview chain instead.
    try {
      const winWithHandler: ESObject = win as ESObject;
      if (typeof winWithHandler.setBackPressedHandler === 'function') {
        winWithHandler.setBackPressedHandler((): void => {
          const mod: ESObject = this.rapidrawModule;
          const notify: ESObject =
            mod?.notifyOhosBackPressed ?? mod?.default?.notifyOhosBackPressed;
          console.info(`[RapidRAW] window back handler fired, notify=${typeof notify}`);
          if (typeof notify === 'function') {
            notify();
          }
        });
        console.info('[RapidRAW] window.setBackPressedHandler registered');
      } else {
        console.warn('[RapidRAW] window.setBackPressedHandler unavailable on this API level');
      }
    } catch (e) {
      console.error(`[RapidRAW] setBackPressedHandler failed: ${JSON.stringify(e)}`);
    }
    // Deterministic keyboard avoidance: RESIZE shrinks the layout so the
    // webview's visualViewport handlers reflow the UI above the IME, the
    // same effect as the Android insets padding in MainActivity.kt. Since
    // API 26 this lives on the window's UIContext (ArkUI), not on Window,
    // and the UIContext only exists after loadContent.
    try {
      win.getUIContext().setKeyboardAvoidMode(KeyboardAvoidMode.RESIZE);
    } catch (e) {
      console.error(`[RapidRAW] setKeyboardAvoidMode failed: ${JSON.stringify(e)}`);
    }
    // Startup user-grant permission requests: deliberately NOT done here —
    // permissions are requested on demand at their actual point of use
    // instead (requestUserPermissionsOnce; triggers live in pickFolder and
    // copyGalleryAsset), per HarmonyOS UX guidance.
    // Keep the system window decoration VISIBLE. We previously hid it
    // (setWindowDecorVisible(false) retry loop) so the custom webview
    // titlebar could own the whole window, but that is unsafe: hiding the
    // decor only removes the visuals — its window-button INPUT rects stay
    // active inside the content area, offset ~one button left of where our
    // webview buttons render, so taps got misrouted to the wrong system op
    // (tapping our maximize icon closed the app). With the decor visible the
    // rects live in the system bar where they belong; the frontend hides its
    // own button cluster on OHOS (TitleBar.tsx) and window dragging goes
    // through the startMoving bridge. Machine-local post-init patch — see
    // docs/HARMONYOS_PORTING.md 6.4.
  }

  // On-demand user-grant permission request, one attempt per permission per
  // session (the system additionally remembers refusals permanently). The
  // resolved promise never rejects: a failed or refused request degrades the
  // calling feature silently instead of breaking it.
  //
  // Trigger points (§6.43 on-demand revision):
  //   - MEDIA_LOCATION: right before the MediaAssetManager.requestImageData
  //     cloud-materialization fallback in copyGalleryAsset — it gates the
  //     location/shooting-params EXIF in the prepared bytes; awaited so the
  //     grant (if given) applies to the very request that follows.
  //   - READ_WRITE_{DOWNLOAD,DOCUMENTS,DESKTOP}_DIRECTORY: after a picked
  //     library root resolves inside one of the public user-storage
  //     directories (directoryPermissionForPath) — a granted blanket
  //     permission frees later launches from the per-launch fileShare
  //     activation chain; fire-and-forget.
  //   - WRITE_IMAGEVIDEO is deliberately never requested: the gallery-save
  //     flow uses the user-consent creation dialog and needs no permission.
  private requestUserPermissionsOnce(permissions: Array<Permissions>): Promise<void> {
    const pending: Array<Permissions> =
      permissions.filter((permission: Permissions): boolean => !this.requestedPermissions.has(permission));
    if (pending.length === 0) {
      return Promise.resolve();
    }
    pending.forEach((permission: Permissions): void => {
      this.requestedPermissions.add(permission);
    });
    try {
      const atManager = abilityAccessCtrl.createAtManager();
      return atManager
        .requestPermissionsFromUser(this.context, pending)
        .then((result: ESObject) => {
          const statuses: Array<number> = result.grantStatus as Array<number>;
          const granted: Array<string> = [];
          const denied: Array<string> = [];
          pending.forEach((permission: string, index: number) => {
            if (statuses[index] === 0) {
              granted.push(permission);
            } else {
              denied.push(permission);
            }
          });
          console.info(
            `[RapidRAW] on-demand permissions: granted=${JSON.stringify(granted)} denied=${JSON.stringify(denied)}`,
          );
        })
        .catch((e: BusinessError) => {
          console.error(`[RapidRAW] requestPermissionsFromUser failed: ${JSON.stringify(e)}`);
        });
    } catch (e) {
      console.error(`[RapidRAW] requestPermissionsFromUser threw: ${JSON.stringify(e)}`);
      return Promise.resolve();
    }
  }

  // Map a picked library root under a public user-storage directory to the
  // blanket directory permission covering that whole tree (both p7b profiles
  // carry all of them in their allowed-acls). A subfolder of Download is
  // covered by the DOWNLOAD permission just as well as the root.
  private directoryPermissionForPath(path: string): Permissions | null {
    const normalized: string = path.endsWith('/') ? path : `${path}/`;
    if (normalized.includes('/Download/')) {
      return 'ohos.permission.READ_WRITE_DOWNLOAD_DIRECTORY';
    }
    if (normalized.includes('/Documents/')) {
      return 'ohos.permission.READ_WRITE_DOCUMENTS_DIRECTORY';
    }
    if (normalized.includes('/Desktop/')) {
      return 'ohos.permission.READ_WRITE_DESKTOP_DIRECTORY';
    }
    return null;
  }

  // Machine-local post-init patch — see docs/HARMONYOS_PORTING.md 6.4.
  private async registerRapidrawWindowController(win: window.Window): Promise<void> {
    try {
      const libName: string = 'liblightraw.so';
      const mod: ESObject = await import(libName);
      const register: ESObject =
        mod.registerOhosWindowController ?? mod.default?.registerOhosWindowController;
      if (typeof register !== 'function') {
        console.error('[RapidRAW] registerOhosWindowController not exported by liblightraw.so');
        return;
      }
      const resolvePickFolder: ESObject =
        mod.resolveOhosPickFolder ?? mod.default?.resolveOhosPickFolder;
      const resolvePickFiles: ESObject =
        mod.resolveOhosPickFiles ?? mod.default?.resolveOhosPickFiles;
      const resolvePickFilesEx: ESObject =
        mod.resolveOhosPickFilesEx ?? mod.default?.resolveOhosPickFilesEx;
      const resolveSaveToGallery: ESObject =
        mod.resolveOhosSaveToGallery ?? mod.default?.resolveOhosSaveToGallery;
      const resolveSaveFileAs: ESObject =
        mod.resolveOhosSaveFileAs ?? mod.default?.resolveOhosSaveFileAs;
      const resolvePrivacy: ESObject =
        mod.resolveOhosPrivacy ?? mod.default?.resolveOhosPrivacy;
      const resolveOpenUrl: ESObject =
        mod.resolveOhosOpenUrl ?? mod.default?.resolveOhosOpenUrl;
      const resolveRevokeGrants: ESObject =
        mod.resolveOhosRevokeGrants ?? mod.default?.resolveOhosRevokeGrants;
      if (typeof resolvePickFolder !== 'function' || typeof resolvePickFiles !== 'function' ||
        typeof resolveSaveToGallery !== 'function' || typeof resolveSaveFileAs !== 'function') {
        console.error('[RapidRAW] bridge resolve exports missing from liblightraw.so');
      }
      if (typeof resolvePickFilesEx !== 'function') {
        console.error('[RapidRAW] resolveOhosPickFilesEx missing from liblightraw.so');
      }
      if (typeof resolvePrivacy !== 'function') {
        console.error('[RapidRAW] resolveOhosPrivacy missing from liblightraw.so');
      }
      if (typeof resolveOpenUrl !== 'function') {
        console.error('[RapidRAW] resolveOhosOpenUrl missing from liblightraw.so');
      }
      if (typeof resolveRevokeGrants !== 'function') {
        console.error('[RapidRAW] resolveOhosRevokeGrants missing from liblightraw.so');
      }
      register((op: string) => {
        try {
          // JSON payloads ({ op, requestId, ... }) route to the bridge
          // handlers; plain op strings are the legacy window controls.
          let req: Record<string, Object> | null = null;
          try {
            req = JSON.parse(op) as Record<string, Object>;
          } catch (e) {
            req = null;
          }
          if (req !== null && typeof req['op'] === 'string') {
            this.handleBridgeRequest(req, resolvePickFolder, resolvePickFiles, resolvePickFilesEx,
              resolveSaveToGallery, resolveSaveFileAs, resolvePrivacy, resolveOpenUrl, resolveRevokeGrants);
            return;
          }
          switch (op) {
            case 'minimize':
              win.minimize();
              break;
            case 'maximize':
              win.maximize().catch((e: BusinessError) => {
                console.error(`[RapidRAW] window op 'maximize' failed: ${JSON.stringify(e)}`);
              });
              break;
            case 'restore':
              win.restore();
              break;
            case 'start_drag':
              win.startMoving();
              break;
            case 'keep_screen_on':
              win.setWindowKeepScreenOn(true).catch((e: BusinessError) => {
                console.error(`[RapidRAW] setWindowKeepScreenOn(true) failed: ${JSON.stringify(e)}`);
              });
              break;
            case 'keep_screen_off':
              win.setWindowKeepScreenOn(false).catch((e: BusinessError) => {
                console.error(`[RapidRAW] setWindowKeepScreenOn(false) failed: ${JSON.stringify(e)}`);
              });
              break;
            case 'exit_app':
              this.context.terminateSelf().catch((e: BusinessError) => {
                console.error(`[RapidRAW] window op 'exit_app' failed: ${JSON.stringify(e)}`);
              });
              break;
            default:
              console.warn(`[RapidRAW] unknown window op '${op}'`);
          }
        } catch (e) {
          console.error(`[RapidRAW] window op '${op}' failed: ${JSON.stringify(e)}`);
        }
      });
      // Keep the module handle for onConfigurationUpdate pushes, and seed
      // the Rust bridge with the current system color mode + form factor.
      this.rapidrawModule = mod;
      this.pushColorMode(this.context.config?.colorMode);
      this.pushDeviceType();
      this.pushAvoidAreas(win);
      if (this.pendingOpenPath !== null) {
        const pendingPath: string = this.pendingOpenPath;
        this.pendingOpenPath = null;
        this.deliverOpenFile(pendingPath);
      }
      // Subscribe only now that the module handle exists: the consent
      // subscription pushes through notifyOhosPrivacyChanged below.
      this.subscribePrivacyEvents();
      this.startTetherUsbBridge();
      this.subscribeUsbHotplug();
      // Seed the auto-download scheduler conditions now that the module
      // handle exists; the wifi/screen subscriptions re-push on changes.
      this.pushNetworkState();
      this.subscribeAutoDownloadWifi();
      this.subscribeScreenEvents();
      console.info('[RapidRAW] OHOS window controller registered');
    } catch (e) {
      console.error(`[RapidRAW] window-controller registration failed: ${JSON.stringify(e)}`);
    }
  }

  // ---- Idle auto-download bridge (docs/AI_MODEL_AUTO_DOWNLOAD.md) ----
  //
  // Pushes the two condition groups the Rust scheduler needs: network
  // state (wifiManager: Wi-Fi up + connected + hotspot off) and idle
  // signals (window focus / ability lifecycle / screen on-off). Rust
  // owns thresholds and the state machine; the webview only renders the
  // resulting ai-auto-download events.
  private screenSubscriber: commonEventManager.CommonEventSubscriber | null = null;
  private continuousTaskWanted: boolean = false;

  private pushNetworkState(): void {
    const mod: ESObject | null = this.rapidrawModule;
    const notify: ESObject = mod?.notifyOhosNetworkState ?? mod?.default?.notifyOhosNetworkState;
    if (typeof notify !== 'function') {
      return; // module not loaded yet; the subscriptions below re-push
    }
    let wifiActive: boolean = false;
    let connected: boolean = false;
    let hotspotActive: boolean = false;
    try {
      wifiActive = wifiManager.isWifiActive();
    } catch (e) {
      console.error(`[RapidRAW] isWifiActive failed: ${JSON.stringify(e)}`);
    }
    try {
      connected = wifiManager.isConnected();
    } catch (e) {
      console.error(`[RapidRAW] isConnected failed: ${JSON.stringify(e)}`);
    }
    try {
      hotspotActive = wifiManager.isHotspotActive();
    } catch (e) {
      console.error(`[RapidRAW] isHotspotActive failed: ${JSON.stringify(e)}`);
    }
    notify(wifiActive, connected, hotspotActive);
  }

  private pushIdleEvent(kind: string, active: boolean): void {
    const mod: ESObject | null = this.rapidrawModule;
    const notify: ESObject = mod?.notifyOhosIdleEvent ?? mod?.default?.notifyOhosIdleEvent;
    if (typeof notify !== 'function') {
      return; // pre-module events are covered by the next push
    }
    notify(kind, active);
  }

  private subscribeAutoDownloadWifi(): void {
    const rePush = (): void => {
      this.pushNetworkState();
    };
    try {
      wifiManager.on('wifiStateChange', rePush);
      wifiManager.on('wifiConnectionChange', rePush);
      wifiManager.on('hotspotStateChange', rePush);
      console.info('[RapidRAW] auto-download wifi subscriptions registered');
    } catch (e) {
      console.error(`[RapidRAW] wifi subscribe failed: ${JSON.stringify(e)}`);
    }
  }

  private subscribeScreenEvents(): void {
    // Screen-off is idle by definition and bypasses the threshold on the
    // Rust side; subscribe both directions so the clock resets on wake.
    const info: commonEventManager.CommonEventSubscribeInfo =
      { events: ['usual.event.SCREEN_ON', 'usual.event.SCREEN_OFF'] };
    commonEventManager.createSubscriber(info)
      .then((subscriber: commonEventManager.CommonEventSubscriber) => {
        this.screenSubscriber = subscriber;
        try {
          commonEventManager.subscribe(subscriber, (err, data) => {
            if (err) {
              console.error(`[RapidRAW] screen event failed: ${JSON.stringify(err)}`);
              return;
            }
            const screenOn: boolean = data?.event === 'usual.event.SCREEN_ON';
            this.pushIdleEvent('screen', screenOn);
            if (screenOn) {
              this.pushNetworkState();
            } else {
              this.holdContinuousTaskIfWanted();
            }
          });
          console.info('[RapidRAW] screen on/off subscribed');
        } catch (e) {
          console.error(`[RapidRAW] screen subscribe threw: ${JSON.stringify(e)}`);
        }
      })
      .catch((e: BusinessError) => {
        console.error(`[RapidRAW] screen subscriber failed: ${JSON.stringify(e)}`);
      });
  }

  // This SDK has no two-argument startBackgroundRunning: every overload
  // needs a WantAgent (or the API-21 ContinuousTaskRequest class). A plain
  // self-referencing WantAgent satisfies it — tapping the task notification
  // just brings the app forward.
  // Synchronous pre-check over the Rust bridge: hold the dataTransfer
  // task only when the feature is enabled, the network preconditions
  // hold and something is actually pending (cheap: cached settings,
  // cached network state, 22 file stats).
  private holdContinuousTaskIfWanted(): void {
    const mod: ESObject | null = this.rapidrawModule;
    const wanted: ESObject = mod?.ohosAutoDownloadWanted ?? mod?.default?.ohosAutoDownloadWanted;
    if (typeof wanted === 'function' && wanted()) {
      this.setAutoDownloadContinuousTask(true).catch(() => undefined);
    }
  }

  private async setAutoDownloadContinuousTask(hold: boolean): Promise<void> {
    if (this.continuousTaskWanted === hold) {
      return;
    }
    this.continuousTaskWanted = hold;
    try {
      if (hold) {
        const agent: WantAgent = await wantAgent.getWantAgent({
          wants: [{
            bundleName: 'io.github.CyberTimon.RapidRAW',
            abilityName: 'EntryAbility',
          }],
          requestCode: 0,
          operationType: wantAgent.OperationType.START_ABILITY,
        });
        await backgroundTaskManager.startBackgroundRunning(this.context,
          backgroundTaskManager.BackgroundMode.DATA_TRANSFER, agent);
        console.info('[RapidRAW] dataTransfer continuous task started');
      } else {
        await backgroundTaskManager.stopBackgroundRunning(this.context);
        console.info('[RapidRAW] dataTransfer continuous task stopped');
      }
    } catch (e) {
      console.error(`[RapidRAW] continuous task ${hold ? 'start' : 'stop'} failed: ${JSON.stringify(e)}`);
    }
  }

  // ---- USB tethering bridge (@ohos.usbManager) -----------------------------
  //
  // HarmonyOS keeps /dev/bus/usb closed to app domains (SELinux
  // dev_bus_usb_file), so libgphoto2 can never open the camera by path. The
  // sanctioned route is usbManager: getDevices -> requestRight (system dialog)
  // -> connectDevice -> getFileDescriptor, and that fd is handed to the Rust
  // side, which wraps it with gp_port_usb_set_sys_device (see
  // src-tauri/src/integrations/tethering.rs). Rust raises a flag through
  // ohosTetherUsbWanted(); this poller answers it.
  private tetherTimer: number = -1;
  // One live pipe per granted device (key: usbManager device name, e.g.
  // "1-3"). The pipe keeps the fd open for Rust; replacing an entry closes
  // the previous pipe so repeated grants cannot leak, and the detach
  // reconcile closes exactly the pipe whose device left the bus.
  private tetherPipes: Map<string, ESObject> = new Map();
  private tetherGranting: boolean = false;
  private tetherAttachBusy: boolean = false;
  private tetherUsbSubscriber: commonEventManager.CommonEventSubscriber | null = null;

  // Sony/Canon/Nikon/Fuji/Panasonic/Hasselblad/Leica/Olympus PTP vendors.
  private static readonly CAMERA_VIDS: Array<number> = [
    0x04a9, 0x054c, 0x04b0, 0x04cb, 0x04da, 0x1a98, 0x1a93, 0x07b4, 0x33b9,
  ];

  private startTetherUsbBridge(): void {
    if (this.tetherTimer !== -1) {
      return;
    }
    this.tetherTimer = setInterval(() => {
      this.pumpTetherUsb();
    }, 700);
    // One-shot probe shortly after startup. `true` = interactive: an attached
    // camera that has not been authorized yet gets the system USB permission
    // dialog right away, so tethering is usable without opening the panel
    // first. (The panel's own poll stays silent - see grantTetherCameras.)
    setTimeout(() => {
      const mod: ESObject | null = this.rapidrawModule;
      if (mod === null) {
        return;
      }
      this.grantTetherCameras(mod, true).catch((e: BusinessError) => {
        console.error(`[RapidRAW] tether startup probe failed: ${JSON.stringify(e)}`);
      });
    }, 4000);
  }

  private pumpTetherUsb(): void {
    const mod: ESObject | null = this.rapidrawModule;
    if (mod === null || this.tetherGranting) {
      return;
    }
    const wanted: ESObject = mod.ohosTetherUsbWanted ?? mod.default?.ohosTetherUsbWanted;
    if (typeof wanted !== 'function') {
      return;
    }
    let request: boolean = false;
    try {
      request = wanted.call(mod) as boolean;
    } catch (e) {
      return;
    }
    if (request) {
      this.tetherGranting = true;
      this.grantTetherCameras(mod, true).then(() => {
        this.tetherGranting = false;
      }).catch((e: BusinessError) => {
        this.tetherGranting = false;
        console.error(`[RapidRAW] tether USB grant failed: ${JSON.stringify(e)}`);
      });
    }
  }

  private async grantTetherCameras(mod: ESObject, interactive: boolean): Promise<void> {
    const report: ESObject =
      mod.notifyOhosTetherUsbError ?? mod.default?.notifyOhosTetherUsbError;
    const accept: ESObject =
      mod.notifyOhosTetherUsbDevice ?? mod.default?.notifyOhosTetherUsbDevice;
    if (typeof accept !== 'function') {
      return;
    }
    let devices: Array<ESObject> = [];
    try {
      devices = usbManager.getDevices() as Array<ESObject>;
    } catch (e) {
      report?.call(mod, `getDevices failed: ${JSON.stringify(e)}`);
      return;
    }
    const cameras: Array<ESObject> = devices.filter((d: ESObject) => {
      const vid: number = Number(d.vendorId);
      return EntryAbility.CAMERA_VIDS.indexOf(vid) >= 0;
    });
    console.info(`[RapidRAW] tether${interactive ? '' : ' (probe)'}: ${devices.length} USB device(s), ${cameras.length} camera candidate(s)`);
    if (cameras.length === 0) {
      if (interactive) {
        const names: string = devices.map((d: ESObject) => `${Number(d.vendorId).toString(16)}:${Number(d.productId).toString(16)}`)
          .join(',');
        report?.call(mod, `no camera VID among [${names}]`);
      }
      return;
    }
    for (let i = 0; i < cameras.length; i++) {
      const device: ESObject = cameras[i];
      const name: string = String(device.name);
      try {
        if (usbManager.hasRight(name)) {
          // Already authorized (earlier grant or the silent probe): hand the
          // fd over without popping the dialog again.
          this.connectTetherDevice(mod, device, name);
          continue;
        }
        if (!interactive) {
          // Silent probe: an unauthorized camera stays untouched here; the
          // dialog belongs to the explicit connect request.
          console.info(`[RapidRAW] tether (probe): ${name} not authorized yet, skipping`);
          continue;
        }
        const granted: boolean = await usbManager.requestRight(name);
        if (!granted) {
          report?.call(mod, `user denied USB access for ${name}`);
          continue;
        }
        this.connectTetherDevice(mod, device, name);
      } catch (e) {
        report?.call(mod, `${name}: ${JSON.stringify(e)}`);
      }
    }
  }

  private connectTetherDevice(mod: ESObject, device: ESObject, name: string): void {
    const report: ESObject =
      mod.notifyOhosTetherUsbError ?? mod.default?.notifyOhosTetherUsbError;
    const accept: ESObject =
      mod.notifyOhosTetherUsbDevice ?? mod.default?.notifyOhosTetherUsbDevice;
    const pipe: ESObject = usbManager.connectDevice(device);
    // Keep the pipe alive: the fd is only valid while it is open. Close any
    // pipe we already hold for this device first - Rust dups the fd, so the
    // moment a fresh grant lands the previous pipe is redundant.
    const previous: ESObject | undefined = this.tetherPipes.get(name);
    if (previous !== undefined) {
      try {
        usbManager.closePipe(previous);
      } catch (e) {
        // The device may have dropped off the bus between the two grants; a
        // stale pipe dies with it, nothing left to clean up.
      }
    }
    this.tetherPipes.set(name, pipe);
    const fd: number = usbManager.getFileDescriptor(pipe);
    console.info(`[RapidRAW] tether: ${name} fd=${fd}`);
    accept?.call(mod, name, Number(device.vendorId), Number(device.productId), fd);
  }

  // ---- USB hotplug (system common events) ----------------------------------
  //
  // usbManager has no on('attach'/'detach') on this API level; the sanctioned
  // source is the COMMON_EVENT_USB_DEVICE_ATTACHED / DETACHED system common
  // events (no subscriber permission required, API 9). The DETACHED leg is
  // what makes unplugging timely: without it the first sign of a dead camera
  // is a failed probe poll up to three seconds later, and the grant's fd,
  // the ArkTS pipe and the Rust camera session all linger until then.
  //
  // Caveat: dynamic common events are not delivered while the app is
  // backgrounded - the Rust-side poll paths remain the fallback there.
  private subscribeUsbHotplug(): void {
    if (this.tetherUsbSubscriber !== null) {
      return;
    }
    const subscribeInfo: commonEventManager.CommonEventSubscribeInfo = {
      events: [
        'usual.event.hardware.usb.action.USB_DEVICE_ATTACHED',
        'usual.event.hardware.usb.action.USB_DEVICE_DETACHED',
      ],
    };
    commonEventManager.createSubscriber(subscribeInfo)
      .then((subscriber: commonEventManager.CommonEventSubscriber) => {
        this.tetherUsbSubscriber = subscriber;
        try {
          commonEventManager.subscribe(subscriber, (err: BusinessError, data: commonEventManager.CommonEventData) => {
            if (err) {
              console.error(`[RapidRAW] USB hotplug callback failed: ${JSON.stringify(err)}`);
              return;
            }
            const event: string = data?.event ?? '';
            if (event === 'usual.event.hardware.usb.action.USB_DEVICE_ATTACHED') {
              this.onUsbDeviceAttached();
            } else if (event === 'usual.event.hardware.usb.action.USB_DEVICE_DETACHED') {
              this.onUsbDeviceDetached();
            }
          });
          console.info('[RapidRAW] subscribed to USB hotplug events');
        } catch (error) {
          console.error(`[RapidRAW] USB hotplug subscribe threw: ${JSON.stringify(error)}`);
        }
      })
      .catch((error: BusinessError) => {
        console.error(`[RapidRAW] createSubscriber for USB hotplug failed: ${JSON.stringify(error)}`);
      });
  }

  // A device landed on the bus: run the same interactive grant flow as the
  // startup probe, so a camera plugged in while the app runs gets the USB
  // permission dialog right away (grantTetherCameras filters out
  // non-camera vendors - plugging in a mouse must not pop anything).
  private onUsbDeviceAttached(): void {
    const mod: ESObject | null = this.rapidrawModule;
    if (mod === null || this.tetherAttachBusy || this.tetherGranting) {
      return;
    }
    this.tetherAttachBusy = true;
    this.grantTetherCameras(mod, true).then(() => {
      this.tetherAttachBusy = false;
    }).catch((e: BusinessError) => {
      this.tetherAttachBusy = false;
      console.error(`[RapidRAW] tether USB attach grant failed: ${JSON.stringify(e)}`);
    });
  }

  // A device left the bus. The event does not say WHICH device, so reconcile
  // against getDevices(): any pipe we hold whose device name is gone is dead -
  // close its pipe, drop it from the map, and tell Rust (which drops the grant
  // and the camera session and notifies the webview). The short delay lets
  // the USB service settle its device list before we read it.
  private onUsbDeviceDetached(): void {
    if (this.tetherPipes.size === 0) {
      return;
    }
    const mod: ESObject | null = this.rapidrawModule;
    const detached: ESObject = mod !== null
      ? (mod.notifyOhosTetherUsbDetached ?? mod.default?.notifyOhosTetherUsbDetached)
      : null;
    setTimeout(() => {
      let devices: Array<ESObject> = [];
      try {
        devices = usbManager.getDevices() as Array<ESObject>;
      } catch (e) {
        console.error(`[RapidRAW] tether detach reconcile: getDevices failed: ${JSON.stringify(e)}`);
        return;
      }
      const present: Set<string> = new Set(devices.map((d: ESObject) => String(d.name)));
      this.tetherPipes.forEach((pipe: ESObject, name: string) => {
        if (present.has(name)) {
          return;
        }
        this.tetherPipes.delete(name);
        try {
          usbManager.closePipe(pipe);
        } catch (e) {
          // The pipe died with the device; closing is best-effort.
        }
        console.info(`[RapidRAW] tether: device ${name} detached - pipe closed, notifying Rust`);
        if (typeof detached === 'function') {
          try {
            detached.call(mod, name);
          } catch (e) {
            console.error(`[RapidRAW] notifyOhosTetherUsbDetached failed: ${JSON.stringify(e)}`);
          }
        }
      });
    }, 500);
  }

  // Ability lifecycle = the phone idle signal: backgrounded apps get
  // frozen unless they hold a continuous task, so the Rust scheduler
  // holds one for the whole download session (docs/AI_MODEL_AUTO_DOWNLOAD.md).
  onForeground(): void {
    super.onForeground();
    this.pushIdleEvent('lifecycle', true);
    // Re-pull the network verdict: while the ability was frozen the
    // wifiManager subscriptions could not run, so the last push the Rust
    // side saw may have been the transient disconnect from the suspend
    // dance (observed on MOR-M1: the status line stayed "waiting for
    // Wi-Fi" on a connected device after resume).
    this.pushNetworkState();
  }

  onBackground(): void {
    super.onBackground();
    this.pushIdleEvent('lifecycle', false);
    // The continuous task must be requested while the app still counts
    // as transitioning to the background: the suspend round proved a
    // tick-after-the-fact request from an already-backgrounded app is
    // refused and the process freezes with the session wedged. The
    // Rust scheduler releases the task on its next tick when the
    // conditions are not actually met.
    this.holdContinuousTaskIfWanted();
  }

  onConfigurationUpdate(newConfig: Configuration): void {
    super.onConfigurationUpdate(newConfig);
    this.pushColorMode(newConfig.colorMode);
    this.applyBaseColor(newConfig.colorMode);
  }

  // Forward the system color mode into the Rust bridge; the webview receives
  // changes as the 'ohos-color-mode' tauri event (see ohos_integration.rs).
  private pushColorMode(colorMode: number | undefined): void {
    const notify: ESObject =
      this.rapidrawModule?.notifyOhosColorMode ?? this.rapidrawModule?.default?.notifyOhosColorMode;
    if (typeof notify !== 'function') {
      return;
    }
    if (colorMode === ConfigurationConstant.ColorMode.COLOR_MODE_DARK) {
      notify(true);
    } else if (colorMode === ConfigurationConstant.ColorMode.COLOR_MODE_LIGHT) {
      notify(false);
    }
  }

  // ---- theme base color (docs 6.25) ----
  //
  // The webview is created with ArkWeb's backgroundColor = Color.Transparent
  // (tauri.conf.json `transparent: true` -> tauri -> wry -> @ohos-rs/ability
  // DefaultXComponent), and src/styles.css keeps html/body transparent too
  // (the desktop frameless window needs it for rounded corners). ArkWeb paints
  // its component background only where the page is transparent, so every such
  // pixel is the WINDOW surface -- black on OHOS, which is what showed up as
  // black blocks before the first frame / React bundle painted.
  //
  // Paint that surface with the theme's base color instead: dark -> black,
  // otherwise white. The same decision goes to AppStorage so pages/Index.ets
  // can mirror it on the ArkUI layer directly under the webview, which keeps
  // the window covered while the page itself is still transparent.
  private applyBaseColor(colorMode: number | undefined): void {
    const dark: boolean = colorMode === ConfigurationConstant.ColorMode.COLOR_MODE_DARK;
    AppStorage.setOrCreate('ohosBaseDark', dark);
    const win: window.Window | null = this.mainWindow;
    if (win === null) {
      return;
    }
    // setWindowBackgroundColor is void in this SDK (and flagged as not
    // available on phone/tablet/2in1 form factors) - call it best-effort in a
    // try/catch; the Index page's opaque base layer is the actual fix.
    try {
      win.setWindowBackgroundColor(dark ? '#000000' : '#ffffff');
    } catch (e) {
      console.error(`[RapidRAW] setWindowBackgroundColor failed: ${JSON.stringify(e)}`);
    }
  }

  // Forward the device form factor into the Rust bridge. The frontend uses it
  // to choose between the desktop (2in1) and touch-first tablet layouts; the
  // module import resolves after the page may already have loaded, so the Rust
  // side also emits 'ohos-device-type' for the frontend to follow (docs 6.14).
  private pushDeviceType(): void {
    const notify: ESObject =
      this.rapidrawModule?.notifyOhosDeviceType ?? this.rapidrawModule?.default?.notifyOhosDeviceType;
    if (typeof notify !== 'function') {
      console.error('[RapidRAW] notifyOhosDeviceType not exported by liblightraw.so');
      return;
    }
    notify(deviceInfo.deviceType);
  }

  // Native safe-area insets for the webview (phone layout G1): merge the
  // avoid rects that matter for layout (system/cutout top, gesture strip and
  // navigation indicator bottom, cutout/gesture sides) and push them through
  // the Rust bridge. Values are physical px; the webview divides by its CSS
  // device pixel ratio before using them as --safe-* variables. Individual
  // avoid types can be missing per form factor / API level, so each probe is
  // guarded.
  private safeAvoidArea(win: window.Window, type: window.AvoidAreaType): window.AvoidArea | null {
    try {
      return win.getWindowAvoidArea(type);
    } catch (e) {
      return null;
    }
  }

  private pushAvoidAreas(win: window.Window): void {
    const notify: ESObject =
      this.rapidrawModule?.notifyOhosAvoidArea ?? this.rapidrawModule?.default?.notifyOhosAvoidArea;
    if (typeof notify !== 'function') {
      return;
    }
    const system: window.AvoidArea | null = this.safeAvoidArea(win, window.AvoidAreaType.TYPE_SYSTEM);
    const cutout: window.AvoidArea | null = this.safeAvoidArea(win, window.AvoidAreaType.TYPE_CUTOUT);
    const gesture: window.AvoidArea | null = this.safeAvoidArea(win, window.AvoidAreaType.TYPE_SYSTEM_GESTURE);
    const indicator: window.AvoidArea | null =
      this.safeAvoidArea(win, window.AvoidAreaType.TYPE_NAVIGATION_INDICATOR);
    // getWindowAvoidArea reports SCREEN-space rects; with
    // setWindowLayoutFullScreen(false) (real phones) the window already starts
    // below the status bar, so only the part overlapping the WINDOW may pad
    // the webview. Emulators run full-screen (winTop=0) and keep the full
    // inset. Clip every rect to the window rect first.
    const winRect: window.Rect = win.getWindowProperties().windowRect;
    const winTop: number = winRect.top;
    const winBottom: number = winRect.top + winRect.height;
    const topInset = (area: window.AvoidArea | null): number => {
      if (!area) return 0;
      const r: window.Rect = area.topRect;
      return Math.max(0, Math.min(r.top + r.height, winTop + r.height) - Math.max(r.top, winTop));
    };
    const bottomInset = (area: window.AvoidArea | null): number => {
      if (!area) return 0;
      const r: window.Rect = area.bottomRect;
      return Math.max(0, Math.min(winBottom, r.top + r.height) - Math.max(winTop, r.top));
    };
    const top: number = Math.max(topInset(system), topInset(cutout));
    const bottom: number = Math.max(bottomInset(system), bottomInset(gesture), bottomInset(indicator));
    const left: number = Math.max(cutout?.leftRect.width ?? 0, gesture?.leftRect.width ?? 0);
    const right: number = Math.max(cutout?.rightRect.width ?? 0, gesture?.rightRect.width ?? 0);
    notify(top, right, bottom, left, winRect.height);
  }

  // ---- "open with" / share (docs 6.22) ----
  //
  // The file manager launches the app with `ohos.want.action.viewData` and the
  // target in want.uri; the share panel sends `ohos.want.action.sendData` with
  // an `ability.params.stream` array. Both are converted to an absolute path
  // the Rust editor can open (media URIs are copied into the app cache first,
  // because only the URI grant makes them readable).

  private captureOpenWant(want: Want): void {
    const uris: Array<string> = this.collectWantUris(want);
    const uri: string = uris.length > 0 ? uris[0] : '';
    if (uri.length === 0) {
      return;
    }
    console.info(`[RapidRAW] open-with want: action=${want.action ?? ''}, uri=${uri}`);
    if (uri.startsWith('file://media')) {
      this.stageMediaUriForOpen(uri);
      return;
    }
    const path: string | null = this.wantUriToPath(uri);
    if (path === null) {
      console.error(`[RapidRAW] open-with: cannot resolve a path for ${uri}`);
      return;
    }
    this.deliverOpenFile(path);
  }

  private collectWantUris(want: Want): Array<string> {
    const uris: Array<string> = [];
    const uri: string = want.uri ?? '';
    if (uri.length > 0) {
      uris.push(uri);
    }
    const params: Record<string, Object> = want.parameters ?? {};
    const stream: Object | undefined = params['ability.params.stream'];
    if (Array.isArray(stream)) {
      const entries: Array<string> = stream as Array<string>;
      for (const entry of entries) {
        if (typeof entry === 'string' && entry.length > 0) {
          uris.push(entry);
        }
      }
    }
    const single: Object | undefined = params['ability.params.fileUri'];
    if (typeof single === 'string' && single.length > 0) {
      uris.push(single);
    }
    return uris;
  }

  // DocumentViewPicker and the file manager hand out file://docs/... URIs;
  // FileUri.path yields the app-sandbox path directly (readable in-process).
  private wantUriToPath(uri: string): string | null {
    if (uri.startsWith('/')) {
      return uri;
    }
    try {
      const path: string = new fileUri.FileUri(uri).path;
      return path.length > 0 ? path : null;
    } catch (e) {
      console.error(`[RapidRAW] open-with URI resolution failed: ${JSON.stringify(e)}`);
      return null;
    }
  }

  // Gallery shares use file://media/... URIs, which the Rust side cannot open
  // by path; copy the bytes into the same staging root the album import uses so
  // an interrupted run is cleaned up at the next start.
  private stageMediaUriForOpen(uri: string): void {
    const stagingDir: string = `${this.context.cacheDir}/import_staging/open_with`;
    try {
      // accessSync returns a boolean and does NOT throw for missing paths —
      // probing with try/catch never created the dir (see pickGalleryImages).
      if (!fs.accessSync(stagingDir)) {
        fs.mkdirSync(stagingDir, true);
      }
      const name: string = this.galleryAssetName(uri, 0);
      const target: string = `${stagingDir}/${name}`;
      const source: fs.File = fs.openSync(uri, fs.OpenMode.READ_ONLY);
      try {
        fs.copyFileSync(source.fd, target);
      } finally {
        fs.closeSync(source);
      }
      console.info(`[RapidRAW] open-with: staged media request at ${target}`);
      this.deliverOpenFile(target);
    } catch (e) {
      console.error(`[RapidRAW] open-with media staging failed for ${uri}: ${JSON.stringify(e)}`);
    }
  }

  private deliverOpenFile(path: string): void {
    const notify: ESObject =
      this.rapidrawModule?.notifyOhosOpenFile ?? this.rapidrawModule?.default?.notifyOhosOpenFile;
    if (typeof notify !== 'function') {
      // The module import resolves after onCreate; keep the last request and
      // flush it from registerRapidrawWindowController.
      this.pendingOpenPath = path;
      console.info(`[RapidRAW] open-with request deferred until the module loads: ${path}`);
      return;
    }
    try {
      notify(path);
      console.info(`[RapidRAW] open-with request delivered: ${path}`);
    } catch (e) {
      console.error(`[RapidRAW] notifyOhosOpenFile failed: ${JSON.stringify(e)}`);
    }
  }

  // Every handler MUST resolve its request exactly once — the Rust side parks
  // a oneshot channel per request id and has no timeout.
  private handleBridgeRequest(
    req: Record<string, Object>,
    resolvePickFolder: ESObject,
    resolvePickFiles: ESObject,
    resolvePickFilesEx: ESObject,
    resolveSaveToGallery: ESObject,
    resolveSaveFileAs: ESObject,
    resolvePrivacy: ESObject,
    resolveOpenUrl: ESObject,
    resolveRevokeGrants: ESObject,
  ): void {
    const op: string = req['op'] as string;
    const requestId: number = req['requestId'] as number;
    switch (op) {
      case 'pick_folder':
        this.pickFolder(requestId, resolvePickFolder);
        break;
      case 'pick_files':
        this.pickFiles(req, requestId, resolvePickFiles);
        break;
      case 'pick_gallery_images':
        this.pickGalleryImages(req, requestId, resolvePickFiles, resolvePickFilesEx);
        break;
      case 'save_to_gallery':
        this.saveToGallery(req, requestId, resolveSaveToGallery);
        break;
      case 'save_file_as':
        this.saveFileAs(req, requestId, resolveSaveFileAs);
        break;
      case 'privacy_state':
        resolvePrivacy(requestId, this.collectPrivacyState());
        break;
      case 'privacy_request_consent':
        this.requestPrivacyConsent(requestId, resolvePrivacy);
        break;
      case 'privacy_revoke':
        this.revokePrivacyConsent(requestId, resolvePrivacy);
        break;
      case 'open_url':
        this.openUrl(req, requestId, resolveOpenUrl);
        break;
      case 'auto_download_task':
        // Hold/release the dataTransfer continuous task. Fire-and-forget:
        // the Rust scheduler does not park a oneshot for this op.
        this.setAutoDownloadContinuousTask(req['hold'] === true);
        break;
      case 'revoke_folder_grants':
        this.handleRevokeFolderGrants(req, requestId, resolveRevokeGrants);
        break;
      default:
        console.warn(`[RapidRAW] unknown bridge op '${op}'`);
    }
  }

  // ---- external links (docs 6.19) ----
  //
  // Opening an external URL has to go through the ability context: ArkWeb drops
  // `<a target="_blank">` clicks (no onWindowNew handler is registered), and
  // tauri-plugin-shell's own open() path spawns `xdg-open`, which does not
  // exist on HarmonyOS — so links used to do nothing at all. A `viewData` want
  // hands the URL to whichever app the system has registered for it (browser
  // for http/https, mail client for mailto:, dialer for tel:).
  private openUrl(req: Record<string, Object>, requestId: number, resolveOpenUrl: ESObject): void {
    const url: string = (req['url'] as string) ?? '';
    if (url.length === 0) {
      resolveOpenUrl(requestId, false, 'empty url');
      return;
    }
    const want: Want = {
      action: 'ohos.want.action.viewData',
      uri: url,
    };
    // Every request must be resolved exactly once (the Rust side parks a
    // oneshot per id and has no timeout), so a synchronous throw from
    // startAbility has to resolve here too.
    try {
      this.context.startAbility(want)
        .then(() => {
          console.info(`[RapidRAW] open_url ok: ${url}`);
          resolveOpenUrl(requestId, true, null);
        })
        .catch((error: BusinessError) => {
          console.error(`[RapidRAW] open_url failed: ${JSON.stringify(error)}`);
          resolveOpenUrl(requestId, false, `startAbility failed: ${error.code}`);
        });
    } catch (error) {
      console.error(`[RapidRAW] open_url threw: ${JSON.stringify(error)}`);
      resolveOpenUrl(requestId, false, 'startAbility threw');
    }
  }

  // ---- AppGallery privacy management service (标准化隐私声明托管服务) ----
  //
  // The store owns first-launch consent when the standardized hosting service
  // is in use; these handlers only expose the service to the app (query the
  // store-issued links and consent state, re-request consent, revoke it). Every
  // path resolves its request exactly once: the Rust side parks a oneshot per
  // request id with no timeout. The service is unavailable on emulators and in
  // builds without the hosted statement (error 1006700003) — the payload then
  // carries ok=false plus code/error and the Rust side fails open.

  private privacyErrorCode(error: ESObject): number {
    try {
      return (error?.code as number) ?? -1;
    } catch (e) {
      return -1;
    }
  }

  private privacyErrorPayload(prefix: string, error: ESObject): string {
    const code: number = this.privacyErrorCode(error);
    let message: string = '';
    try {
      message = (error?.message as string) ?? '';
    } catch (e) {
      message = '';
    }
    const detail: string = `${prefix}: code=${code}${message !== '' ? `, message=${message}` : ''}`;
    console.error(`[RapidRAW] ${detail}`);
    return JSON.stringify({ 'ok': false, 'error': detail, 'code': code });
  }

  private collectPrivacyState(): string {
    const links: Array<Record<string, Object>> = [];
    const results: Array<Record<string, Object>> = [];
    const errors: Array<string> = [];
    let ok: boolean = false;
    // First AppGallery error code seen, so the app can tell "hosting not in use"
    // (1006700003) apart from a generic failure. Device-verified need: without
    // this the payload carried no code and the About row fell back to "Unknown"
    // instead of "Not enabled".
    let code: number = -1;
    // getAppPrivacyMgmtInfo: the store-issued privacy statement / user
    // agreement links (AppPrivacyLink: type, versionCode, url).
    try {
      const info: privacyManager.AppPrivacyMgmtInfo = privacyManager.getAppPrivacyMgmtInfo();
      const privacyLinks: privacyManager.AppPrivacyLink[] = info.privacyInfo;
      for (let i = 0; i < privacyLinks.length; i++) {
        links.push({
          'type': privacyLinks[i]['type'] ?? 0,
          'versionCode': privacyLinks[i]['versionCode'] ?? 0,
          'url': privacyLinks[i]['url'] ?? '',
        });
      }
      ok = true;
    } catch (error) {
      if (code < 0) { code = this.privacyErrorCode(error); }
      errors.push(`getAppPrivacyMgmtInfo failed: ${JSON.stringify(error)}`);
      console.error(`[RapidRAW] getAppPrivacyMgmtInfo failed: ${JSON.stringify(error)}`);
    }
    // getAppPrivacyResult: the recorded consent verdict per document.
    try {
      const privacyResults: privacyManager.AppPrivacyResult[] = privacyManager.getAppPrivacyResult();
      for (let i = 0; i < privacyResults.length; i++) {
        results.push({
          'type': privacyResults[i]['type'] ?? 0,
          'versionCode': privacyResults[i]['versionCode'] ?? 0,
          'result': privacyResults[i]['result'] ?? 0,
          'signingTimestamp': privacyResults[i]['signingTimestamp'] ?? 0,
        });
      }
      ok = true;
    } catch (error) {
      if (code < 0) { code = this.privacyErrorCode(error); }
      errors.push(`getAppPrivacyResult failed: ${JSON.stringify(error)}`);
      console.error(`[RapidRAW] getAppPrivacyResult failed: ${JSON.stringify(error)}`);
    }
    // ArkTS: every value in a Record<string, Object> must be non-nullable, so the
    // error field is '' rather than null (the Rust side treats '' as absent).
    const errorText: string = errors.length > 0 ? errors.join('; ') : '';
    const payload: Record<string, Object> = {
      'ok': ok,
      'links': links,
      'results': results,
      'error': errorText,
      'code': code,
    };
    const json: string = JSON.stringify(payload);
    console.info(`[RapidRAW] privacy state: ${json}`);
    return json;
  }

  private requestPrivacyConsent(requestId: number, resolvePrivacy: ESObject): void {
    try {
      privacyManager.requestAppPrivacyConsent(this.context as common.UIAbilityContext)
        .then((consentResult: privacyManager.ConsentResult) => {
          const results: Array<Record<string, Object>> = [];
          const privacyResults: privacyManager.AppPrivacyResult[] = consentResult['results'] ?? [];
          for (let i = 0; i < privacyResults.length; i++) {
            results.push({
              'type': privacyResults[i]['type'] ?? 0,
              'versionCode': privacyResults[i]['versionCode'] ?? 0,
              'result': privacyResults[i]['result'] ?? 0,
              'signingTimestamp': privacyResults[i]['signingTimestamp'] ?? 0,
            });
          }
          const payload: Record<string, Object> = {
            'ok': true,
            'links': [],
            'results': results,
            'error': '',
          };
          // Only carry isAgreed when the platform actually reports a boolean:
          // the Rust side prefers it over results[] when present.
          const agreed: boolean | undefined = consentResult['isAgreed'];
          if (typeof agreed === 'boolean') {
            payload['isAgreed'] = agreed;
          }
          const json: string = JSON.stringify(payload);
          console.info(`[RapidRAW] privacy consent result: ${json}`);
          resolvePrivacy(requestId, json);
        })
        .catch((error: BusinessError) => {
          resolvePrivacy(requestId, this.privacyErrorPayload('requestAppPrivacyConsent failed', error));
        });
    } catch (error) {
      resolvePrivacy(requestId, this.privacyErrorPayload('requestAppPrivacyConsent threw', error));
    }
  }

  // Revoking must be *verified*: disableService() is a void call — it reports
  // nothing — so the only proof that the record is gone is reading it back.
  // Without that check the app answered {ok:true} unconditionally and the About
  // row claimed "not accepted" even where the hosted service is not in use
  // (1006700003) or the platform kept the signature: a revoke that silently did
  // nothing, which after a restart looked like the revoke never worked (docs
  // 6.20). Every path resolves the parked request exactly once.
  private revokePrivacyConsent(requestId: number, resolvePrivacy: ESObject): void {
    try {
      privacyManager.disableService();
    } catch (error) {
      resolvePrivacy(requestId, this.privacyErrorPayload('disableService failed', error));
      return;
    }

    try {
      const results: privacyManager.AppPrivacyResult[] = privacyManager.getAppPrivacyResult();
      const signed: Array<Record<string, Object>> = [];
      for (let i = 0; i < results.length; i++) {
        const verdict: number = (results[i]['result'] as number) ?? 0;
        if (verdict === 1) {
          signed.push({
            'type': results[i]['type'] ?? 0,
            'result': verdict,
          });
        }
      }
      if (signed.length > 0) {
        console.error('[RapidRAW] revoke did not clear the consent record');
        resolvePrivacy(requestId, JSON.stringify({
          'ok': false,
          'results': signed,
          'error': 'the consent record is still signed after disableService',
          'code': 0,
        }));
        return;
      }
      console.info('[RapidRAW] privacy consent revoked');
      resolvePrivacy(requestId, JSON.stringify({ 'ok': true, 'results': [], 'error': '', 'code': 0 }));
    } catch (error) {
      // Hosting not in use (1006700003) or the service refused: there is no
      // record to revoke, and saying so is the truthful answer.
      resolvePrivacy(requestId, this.privacyErrorPayload('getAppPrivacyResult after revoke failed', error));
    }
  }

  // The store sends PRIVACY_STATE_CHANGED after the user answers its
  // standardized dialog; forward it so the webview can react without polling.
  private subscribePrivacyEvents(): void {
    const subscribeInfo: commonEventManager.CommonEventSubscribeInfo = {
      events: ['usual.event.PRIVACY_STATE_CHANGED'],
    };
    commonEventManager.createSubscriber(subscribeInfo)
      .then((subscriber: commonEventManager.CommonEventSubscriber) => {
        this.privacySubscriber = subscriber;
        try {
          commonEventManager.subscribe(subscriber, (err: BusinessError, data: commonEventManager.CommonEventData) => {
            if (err) {
              console.error(`[RapidRAW] privacy event subscribe failed: ${JSON.stringify(err)}`);
              return;
            }
            let resultType: number = 0;
            let appIndex: number = 0;
            try {
              const parsed: Record<string, number> = JSON.parse(data?.data ?? '{}') as Record<string, number>;
              resultType = parsed['resultType'] ?? 0;
              appIndex = parsed['appIndex'] ?? 0;
            } catch (e) {
              console.error(`[RapidRAW] privacy event payload parse failed: ${JSON.stringify(e)}`);
            }
            console.info(`[RapidRAW] privacy state changed: resultType=${resultType}, appIndex=${appIndex}`);
            const notify: ESObject =
              this.rapidrawModule?.notifyOhosPrivacyChanged ?? this.rapidrawModule?.default?.notifyOhosPrivacyChanged;
            if (typeof notify === 'function') {
              notify(JSON.stringify({ 'granted': resultType === 1, 'appIndex': appIndex }));
            }
          });
          console.info('[RapidRAW] subscribed to PRIVACY_STATE_CHANGED');
        } catch (error) {
          console.error(`[RapidRAW] privacy event subscribe threw: ${JSON.stringify(error)}`);
        }
      })
      .catch((error: BusinessError) => {
        console.error(`[RapidRAW] createSubscriber for privacy events failed: ${JSON.stringify(error)}`);
      });
  }

  // Defensive bound for the system pickers. On some images the picker UI
  // closes without ever resolving its promise: the DevEco 26 emulator's
  // DocumentViewPicker terminates itself ~2s after launch (result length 0,
  // resultCode -1) and its PhotoViewPicker swallows the result after 完成 —
  // the app-side await then pends forever (2026-11-28 emulator forensics:
  // zero bridge logs after the modal closed). A legitimate session always
  // settles when the picker closes, so this generous bound only fires in the
  // broken case, and the warn line names the defect for whoever reads the
  // log next. Never rejects: the caller's fallback completes the flow.
  private boundPickerResult<T>(
    picker: Promise<T>,
    fallback: T,
    label: string,
    ms: number,
  ): Promise<T> {
    return new Promise<T>((resolve: (value: T) => void) => {
      let settled: boolean = false;
      picker
        .then((value: T): void => {
          if (!settled) {
            settled = true;
            resolve(value);
          }
        })
        .catch((e: BusinessError): void => {
          if (!settled) {
            settled = true;
            console.error(`[RapidRAW] ${label} failed: ${JSON.stringify(e)}`);
            resolve(fallback);
          }
        });
      setTimeout(() => {
        if (!settled) {
          settled = true;
          console.warn(
            `[RapidRAW] ${label} did not deliver a result within ${ms}ms ` +
              `(picker closed without resolving - known DevEco 26 emulator defect); falling back`,
          );
          resolve(fallback);
        }
      }, ms);
    });
  }

  private pickFolder(requestId: number, resolvePickFolder: ESObject): void {
    const options: picker.DocumentSelectOptions = new picker.DocumentSelectOptions();
    options.selectMode = picker.DocumentSelectMode.FOLDER;
    options.maxSelectNumber = 1;
    const documentPicker = new picker.DocumentViewPicker(this.context);
    this.boundPickerResult<Array<string>>(documentPicker.select(options), [], 'pick_folder', 180000)
      .then((uris: Array<string>) => {
      let resolved: string | null = null;
      if (uris && uris.length > 0) {
        const rawUri: string = uris[0];
        console.info(`[RapidRAW] pick_folder raw uri: ${rawUri}`);
        try {
          // DocumentViewPicker returns file://docs/... URIs; FileUri.path
          // yields the app-sandbox path directly (verified: readable by the
          // Rust side, same process).
          resolved = new fileUri.FileUri(rawUri).path || null;
          console.info(`[RapidRAW] pick_folder path: ${resolved}`);
        } catch (e) {
          console.error(`[RapidRAW] folder URI path resolution failed: ${JSON.stringify(e)}`);
        }
        if (resolved) {
          try {
            const stat: fs.Stat = fs.statSync(resolved);
            console.info(`[RapidRAW] pick_folder stat ok: isDirectory=${stat.isDirectory()}`);
          } catch (e) {
            console.error(`[RapidRAW] pick_folder stat FAILED for ${resolved}: ${JSON.stringify(e)}`);
          }
          // Convert the picker's temporary grant into a persistent one.
          // Attempted on every form factor (tablets may or may not expose the
          // persistence syscap); a rejection falls back to the session grant.
          // Guarded: grant persistence must never break the pick flow —
          // resolvePickFolder below must always run.
          try {
            this.persistFolderGrant(rawUri, resolved);
          } catch (e) {
            console.error(`[RapidRAW] persistFolderGrant threw: ${JSON.stringify(e)}`);
          }
          // On-demand directory permission (§6.43): a root inside a public
          // user-storage directory is covered blanket by the matching
          // restricted permission (p7b allowed-acls include all of them).
          // Granted → later launches read the folder directly and stop
          // depending on the per-launch fileShare activation chain;
          // refused → nothing changes, the picker grant keeps working.
          const dirPermission: Permissions | null = this.directoryPermissionForPath(resolved);
          if (dirPermission !== null) {
            this.requestUserPermissionsOnce([dirPermission]);
          }
        }
      }
      resolvePickFolder(requestId, resolved);
    }).catch((e: BusinessError) => {
      console.error(`[RapidRAW] folder picker failed: ${JSON.stringify(e)}`);
      resolvePickFolder(requestId, null);
    });
  }

  private pickFiles(req: Record<string, Object>, requestId: number, resolvePickFiles: ESObject): void {
    const extensions: Array<string> = (req['extensions'] as Array<string>) ?? [];
    const maxSelect: number = (req['maxSelect'] as number) ?? 500;
    const options: picker.DocumentSelectOptions = new picker.DocumentSelectOptions();
    options.maxSelectNumber = Math.min(maxSelect, 500);
    if (extensions.length > 0) {
      // Suffix filters use the '.ext' form.
      options.fileSuffixFilters = extensions.map((ext: string): string =>
        ext.startsWith('.') ? ext.toLowerCase() : `.${ext.toLowerCase()}`);
      console.info(`[RapidRAW] pick_files filters: ${JSON.stringify(options.fileSuffixFilters)}`);
    }
    const documentPicker = new picker.DocumentViewPicker(this.context);
    this.boundPickerResult<Array<string>>(documentPicker.select(options), [], 'pick_files', 180000)
      .then((uris: Array<string>) => {
      const paths: Array<string> = [];
      if (uris) {
        for (const rawUri of uris) {
          try {
            paths.push(new fileUri.FileUri(rawUri).path || rawUri);
          } catch (e) {
            console.error(`[RapidRAW] file URI path resolution failed for ${rawUri}: ${JSON.stringify(e)}`);
            paths.push(rawUri);
          }
        }
      }
      console.info(`[RapidRAW] pick_files -> ${paths.length} file(s)`);
      resolvePickFiles(requestId, paths);
    }).catch((e: BusinessError) => {
      console.error(`[RapidRAW] file picker failed: ${JSON.stringify(e)}`);
      resolvePickFiles(requestId, []);
    });
  }

  // Import photos from the system gallery (MediaLibraryKit picker). The picker
  // returns media URIs with a permanent read grant; the import pipeline works
  // on regular files, so the bytes are copied into the app cache first.
  // Unsupported extensions and duplicate URIs are dropped before copying, and
  // the caller removes the staged files once the import completes.
  //
  // resolvePickGalleryEx is the error-aware variant of the bridge resolver
  // (ok/paths/error): it is used when none of the picked files could be
  // opened even after retries, so the failure reaches the frontend as an
  // error message instead of an empty, silently-dropped import.
  private pickGalleryImages(
    req: Record<string, Object>,
    requestId: number,
    resolvePickGallery: ESObject,
    resolvePickGalleryEx: ESObject,
  ): void {
    const wanted: Set<string> = new Set<string>();
    const extensions: Array<string> = (req['extensions'] as Array<string>) ?? [];
    for (const ext of extensions) {
      wanted.add((ext.startsWith('.') ? ext : `.${ext}`).toLowerCase());
    }
    const maxSelect: number = (req['maxSelect'] as number) ?? 500;
    const options: photoAccessHelper.PhotoSelectOptions = new photoAccessHelper.PhotoSelectOptions();
    options.MIMEType = photoAccessHelper.PhotoViewMIMETypes.IMAGE_TYPE;
    options.maxSelectNumber = Math.min(maxSelect, 500);

    const photoPicker: photoAccessHelper.PhotoViewPicker = new photoAccessHelper.PhotoViewPicker();
    this.boundPickerResult<photoAccessHelper.PhotoSelectResult>(
      photoPicker.select(options),
      new photoAccessHelper.PhotoSelectResult(),
      'pick_gallery_images',
      180000,
    ).then((result: photoAccessHelper.PhotoSelectResult) => {
      const uris: Array<string> = result.photoUris ?? [];
      if (uris.length === 0) {
        console.info('[RapidRAW] pick_gallery_images cancelled');
        resolvePickGallery(requestId, []);
        return;
      }
      const pickedUris: Array<string> = [];
      const pickedNames: Array<string> = [];
      const seen: Set<string> = new Set<string>();
      for (let i = 0; i < uris.length; i++) {
        const uri: string = uris[i];
        if (seen.has(uri)) {
          continue;
        }
        seen.add(uri);
        const name: string = this.galleryAssetName(uri, i);
        const dot: number = name.lastIndexOf('.');
        const extension: string = dot >= 0 ? name.substring(dot).toLowerCase() : '';
        // AGC review defect: photo picker URIs on newer HarmonyOS builds
        // (e.g. "file://media/photo/12") carry no filename extension, so
        // the whitelist filter silently dropped every selected image and
        // the import chain returned zero files. The picker already
        // guarantees IMAGE_TYPE, so an empty extension is always safe to
        // pass through; only reject files that have a *recognizable*
        // extension that is explicitly not in the whitelist.
        if (wanted.size > 0 && extension.length > 0 && !wanted.has(extension)) {
          console.info(`[RapidRAW] pick_gallery_images skip unsupported ${name}`);
          continue;
        }
        pickedUris.push(uri);
        pickedNames.push(name);
      }
      if (pickedUris.length === 0) {
        resolvePickGallery(requestId, []);
        return;
      }

      const stagingRoot: string = `${this.context.cacheDir}/import_staging`;
      const stagingDir: string = `${stagingRoot}/${requestId}`;
      // CRITICAL: fs.accessSync RETURNS a boolean (false when the path is
      // missing) — it does NOT throw for missing paths. The original
      // try/catch pattern never ran mkdirSync, so the staging directories
      // were never created and every copy to `<stagingDir>/<name>` failed
      // with 13900002 (ENOENT) — the true root cause of the Mate 80
      // "first album import shows nothing" chain (2026-11-25).
      if (!fs.accessSync(stagingRoot)) {
        fs.mkdirSync(stagingRoot, true);
      }
      if (!fs.accessSync(stagingDir)) {
        fs.mkdirSync(stagingDir, true);
      }

      // Precompute the staging targets: keep the original display name so the
      // import filename template ({original_filename}) still produces the
      // expected name; duplicates within one pick get an index suffix; media
      // URIs without a filename (e.g. "file://media/photo/12") get a .jpg
      // fallback — the frontend extension filter needs a recognizable suffix.
      const targets: Array<string> = [];
      const usedNames: Set<string> = new Set<string>();
      for (let i = 0; i < pickedUris.length; i++) {
        let name: string = pickedNames[i];
        if (!name.includes('.')) {
          name = `${name}.jpg`;
        }
        if (usedNames.has(name)) {
          const dot: number = name.lastIndexOf('.');
          const stem: string = dot > 0 ? name.substring(0, dot) : name;
          const extension: string = dot > 0 ? name.substring(dot) : '';
          name = `${stem}_${i}${extension}`;
        }
        usedNames.add(name);
        targets.push(`${stagingDir}/${name}`);
      }

      // Do NOT copy here. fs.openSync on the picked URI inside the select()
      // callback fails with 13900002 (ENOENT) on current HarmonyOS builds —
      // the grant only becomes usable after the picker callback returns. A
      // previous pick parking in this slot cannot overlap the modal picker,
      // but settle it defensively so its Rust oneshot never hangs.
      if (this.galleryPickResolver !== null) {
        const staleResolver: ESObject = this.galleryPickResolver;
        const staleId: number = this.galleryPickRequestId;
        const staleStaged: Array<string> = this.galleryPickStaged;
        this.galleryPickResolver = null;
        console.warn('[RapidRAW] pick_gallery_images: settling an earlier pick before staging a new one');
        staleResolver(staleId, staleStaged);
      }
      this.galleryPickRequestId = requestId;
      this.galleryPickResolver = resolvePickGallery;
      this.galleryPickUris = pickedUris;
      this.galleryPickTargets = targets;
      this.galleryPickStaged = [];
      this.galleryPickCopied = pickedUris.map((ignored: string): boolean => false);
      this.galleryPickAttempt = 0;
      console.info(`[RapidRAW] pick_gallery_images picked ${pickedUris.length}; deferring staging out of the picker callback`);
      setTimeout(() => {
        this.stageGalleryImages(resolvePickGalleryEx).catch((e: BusinessError) => {
          console.error(`[RapidRAW] stageGalleryImages failed: ${JSON.stringify(e)}`);
        });
      }, EntryAbility.GALLERY_PICK_RETRY_DELAYS_MS[0]);
    }).catch((e: BusinessError) => {
      console.error(`[RapidRAW] gallery picker failed: ${JSON.stringify(e)}`);
      resolvePickGallery(requestId, []);
    });
  }

  // Copy the parked gallery pick into the staging directory, one timer hop
  // outside the picker's select() callback (see pickGalleryImages). Each
  // attempt retries only the files that have not been copied yet; if some
  // still fail and attempts remain, re-schedule with a longer back-off. The
  // parked request is resolved exactly once, with the staged paths (partial
  // success imports what worked) or — when nothing could be staged — through
  // the error-aware resolver so the import chain surfaces a failure instead
  // of silently returning zero files.
  private async stageGalleryImages(resolvePickGalleryEx: ESObject): Promise<void> {
    const resolver: ESObject | null = this.galleryPickResolver;
    if (resolver === null) {
      return;
    }
    const requestId: number = this.galleryPickRequestId;
    const uris: Array<string> = this.galleryPickUris;
    const targets: Array<string> = this.galleryPickTargets;
    const copied: Array<boolean> = this.galleryPickCopied;
    try {
      this.galleryPickAttempt += 1;
      const attempt: number = this.galleryPickAttempt;
      const startedAt: number = Date.now();
      let remaining: number = 0;
      for (let i = 0; i < uris.length; i++) {
        if (copied[i]) {
          continue;
        }
        const viaMediaRequest: boolean = await this.copyGalleryAsset(uris[i], targets[i]);
        if (viaMediaRequest) {
          copied[i] = true;
          this.galleryPickStaged.push(targets[i]);
        } else {
          remaining += 1;
        }
      }
      const staged: Array<string> = this.galleryPickStaged;
      if (remaining === 0) {
        console.info(
          `[RapidRAW] pick_gallery_images staged ${staged.length}/${uris.length} on attempt ${attempt} in ${Date.now() - startedAt}ms`,
        );
        this.galleryPickResolver = null;
        resolver(requestId, staged);
        return;
      }
      if (attempt < EntryAbility.GALLERY_PICK_MAX_ATTEMPTS) {
        const delay: number = EntryAbility.GALLERY_PICK_RETRY_DELAYS_MS[attempt];
        console.warn(`[RapidRAW] pick_gallery_images: ${remaining} file(s) not copyable yet, retrying in ${delay}ms`);
        setTimeout(() => {
          this.stageGalleryImages(resolvePickGalleryEx).catch((e: BusinessError) => {
            console.error(`[RapidRAW] stageGalleryImages retry failed: ${JSON.stringify(e)}`);
          });
        }, delay);
        return;
      }
      console.error(
        `[RapidRAW] pick_gallery_images gave up: staged ${staged.length}/${uris.length} after ${attempt} attempts`,
      );
      this.galleryPickResolver = null;
      if (staged.length === 0) {
        resolvePickGalleryEx(
          requestId,
          false,
          staged,
          'Failed to open the picked gallery images after several attempts. Please try importing again.',
        );
      } else {
        resolver(requestId, staged);
      }
    } catch (e) {
      // Never leave the parked Rust oneshot hanging on an internal error.
      console.error(`[RapidRAW] stageGalleryImages failed: ${JSON.stringify(e)}`);
      this.galleryPickResolver = null;
      resolvePickGalleryEx(
        requestId,
        false,
        this.galleryPickStaged,
        `Gallery import staging failed: ${JSON.stringify(e)}`,
      );
    }
  }

  // Copy one picked media URI into its staging target. Two paths, in order:
  // 1. Fast path — fs.openSync on the picker-granted URI + copyFileSync.
  //    This is the documented default and keeps the ORIGINAL file bytes
  //    (full EXIF). It is what older HarmonyOS builds (tablets/2in1 where the
  //    album import was first verified) resolve natively.
  // 2. Fallback — MediaAssetManager.requestImageData (official "指定URI获取
  //    图片或视频资源" recipe). On HarmonyOS 6 (Mate 80, 2026-11-25) fs.open
  //    fails with 13900002 (ENOENT) even seconds after the pick and fully
  //    outside the picker callback — for moving photos (logical bundle dir in
  //    the URI, e.g. file://media/Photo/4/IMG_..._003/IMG_...jpg) and for
  //    cloud-synced assets alike; the fs layer simply cannot dereference
  //    those logical URIs. requestImageData handles both (it prepares the
  //    resource, downloading cloud assets on demand) and needs no extra
  //    permission for picker-granted URIs. Cost: EXIF location/shooting
  //    params are privacy-sanitized in this path (MEDIA_LOCATION-gated).
  // Returns true when the target exists with the asset bytes.
  private async copyGalleryAsset(uri: string, target: string): Promise<boolean> {
    try {
      const source: fs.File = fs.openSync(uri, fs.OpenMode.READ_ONLY);
      let sourceSize: number = 0;
      try {
        sourceSize = fs.statSync(source.fd).size;
        fs.copyFileSync(source.fd, target);
      } finally {
        fs.closeSync(source);
      }
      // HarmonyOS "optimized storage" offloads old gallery assets to the
      // cloud and replaces the local file with a 0-byte stub: openSync +
      // copyFileSync succeed but copy nothing. Staging such a file would
      // import as a permanent broken entry (no thumbnail, the editor cannot
      // open it; observed on Mate 80, 2026-11-25). Treat an empty result —
      // or ANY size mismatch, which is what a lazily-materialized cloud
      // file yields when the FUSE read returns EOF early without an error
      // (Mate 80, 2026-11-25 second round: a partially staged screenshot
      // imported as a grey placeholder failing "Failed to decode image") —
      // as a fast-path miss so the MediaAssetManager fallback below runs:
      // it materialises the complete cloud bytes.
      const stagedSize: number = fs.statSync(target).size;
      if (sourceSize > 0 && stagedSize === sourceSize) {
        console.info(`[RapidRAW] gallery copy via fs fd: ${target} (${stagedSize} bytes)`);
        return true;
      }
      console.warn(
        `[RapidRAW] gallery fast-path copy yielded ${stagedSize} bytes (source ${sourceSize}) for ${uri}; trying MediaAssetManager`,
      );
      try {
        fs.unlinkSync(target);
      } catch (cleanupError) {
        // Best effort - the retry loop overwrites the target anyway.
      }
    } catch (e) {
      console.warn(`[RapidRAW] gallery fs open failed for ${uri} (${JSON.stringify(e)}); trying MediaAssetManager`);
    }

    // MEDIA_LOCATION gates the location/shooting-params EXIF in the bytes
    // requestImageData prepares: without it they are privacy-sanitized.
    // Requested on demand here — once per session, and awaited so a granted
    // permission applies to the very request that follows (§6.43). A refusal
    // only means location-less EXIF, never a failed import.
    await this.requestUserPermissionsOnce(['ohos.permission.MEDIA_LOCATION']);

    try {
      const helper: photoAccessHelper.PhotoAccessHelper = photoAccessHelper.getPhotoAccessHelper(this.context);
      const predicates: dataSharePredicates.DataSharePredicates = new dataSharePredicates.DataSharePredicates();
      predicates.equalTo(photoAccessHelper.PhotoKeys.URI, uri);
      const fetchOptions: photoAccessHelper.FetchOptions = {
        fetchColumns: [photoAccessHelper.PhotoKeys.TITLE],
        predicates: predicates,
      };
      const fetchResult: photoAccessHelper.FetchResult<photoAccessHelper.PhotoAsset> = await helper.getAssets(fetchOptions);
      try {
        const photoAsset: photoAccessHelper.PhotoAsset = await fetchResult.getFirstObject();
        const requestOptions: photoAccessHelper.RequestOptions = {
          deliveryMode: photoAccessHelper.DeliveryMode.HIGH_QUALITY_MODE,
        };
        const data: ArrayBuffer | null = await new Promise<ArrayBuffer | null>((resolve) => {
          const handler: GalleryAssetDataHandler = new GalleryAssetDataHandler((prepared: ArrayBuffer | null) => {
            resolve(prepared);
          });
          photoAccessHelper.MediaAssetManager.requestImageData(this.context, photoAsset, requestOptions, handler)
            .catch((err: BusinessError) => {
              console.error(`[RapidRAW] requestImageData failed for ${uri}: ${JSON.stringify(err)}`);
              resolve(null);
            });
        });
        if (data === null || data.byteLength === 0) {
          console.error(`[RapidRAW] gallery media request returned no data for ${uri}`);
          return false;
        }
        const file: fs.File = fs.openSync(target, fs.OpenMode.WRITE_ONLY | fs.OpenMode.CREATE | fs.OpenMode.TRUNC);
        try {
          fs.writeSync(file.fd, data);
        } finally {
          fs.closeSync(file);
        }
        console.info(`[RapidRAW] gallery copy via MediaAssetManager: ${target} (${data.byteLength} bytes)`);
        return true;
      } finally {
        fetchResult.close();
      }
    } catch (e) {
      console.error(`[RapidRAW] gallery media request copy failed for ${uri}: ${JSON.stringify(e)}`);
      return false;
    }
  }

  // Display name for a media URI: strip the scheme/path, decode percent
  // escapes and replace path separators so the staging file cannot escape its
  // directory.
  private galleryAssetName(uri: string, index: number): string {
    let name: string = uri;
    const slash: number = uri.lastIndexOf('/');
    if (slash >= 0) {
      name = uri.substring(slash + 1);
    }
    try {
      name = decodeURIComponent(name);
    } catch (e) {
      // Keep the raw name when the URI is not valid percent-encoding.
    }
    let cut: number = name.indexOf('?');
    const hash: number = name.indexOf('#');
    if (cut < 0 || (hash >= 0 && hash < cut)) {
      cut = hash;
    }
    if (cut >= 0) {
      name = name.substring(0, cut);
    }
    name = name.split('\\').join('_').split('/').join('_');
    return name.length > 0 ? name : `image_${index}`;
  }

  // Remove leftovers from an interrupted import at startup. Staging uses one
  // directory per pick, so clear the files in each and drop the directory
  // (rmdirSync is not recursive).
  private cleanupImportStaging(): void {
    const dir: string = `${this.context.cacheDir}/import_staging`;
    try {
      const entries: Array<string> = fs.listFileSync(dir);
      let removed: number = 0;
      for (const entry of entries) {
        const requestDir: string = `${dir}/${entry}`;
        try {
          const files: Array<string> = fs.listFileSync(requestDir);
          for (const file of files) {
            try {
              fs.unlinkSync(`${requestDir}/${file}`);
              removed++;
            } catch (e) {
              // Ignore a single stuck entry.
            }
          }
          fs.rmdirSync(requestDir);
        } catch (e) {
          // A flat file left by an older build: delete it directly.
          try {
            fs.unlinkSync(requestDir);
            removed++;
          } catch (unlinkError) {
            // Ignore.
          }
        }
      }
      fs.rmdirSync(dir);
      console.info(`[RapidRAW] cleared ${removed} staged import file(s)`);
    } catch (e) {
      // Directory absent on a clean first launch.
    }
  }

  private saveToGallery(req: Record<string, Object>, requestId: number, resolveSaveToGallery: ESObject): void {
    const tempPath: string = req['tempPath'] as string;
    const fileName: string = req['fileName'] as string;
    const dotIndex: number = fileName.lastIndexOf('.');
    const extension: string = dotIndex >= 0 ? fileName.substring(dotIndex + 1).toLowerCase() : 'jpg';
    const title: string = dotIndex >= 0 ? fileName.substring(0, dotIndex) : fileName;
    console.info(`[RapidRAW] save_to_gallery tempPath: ${tempPath}`);
    // Stage the bytes into the sandbox first: both createAsset and
    // showAssetsCreationDialog need a file the app can open.
    const sandboxDir: string = `${this.context.cacheDir}/export_bridge`;
    const stagedName: string = tempPath.substring(tempPath.lastIndexOf('/') + 1);
    const sandboxPath: string = `${sandboxDir}/${stagedName}`;
    try {
      if (!fs.accessSync(sandboxDir)) {
        fs.mkdirSync(sandboxDir, true);
      }
      fs.copyFileSync(tempPath, sandboxPath);
    } catch (e) {
      console.error(`[RapidRAW] save_to_gallery staging failed: ${JSON.stringify(e)}`);
      resolveSaveToGallery(requestId, false, `staging failed: ${JSON.stringify(e)}`);
      return;
    }
    const helper: photoAccessHelper.PhotoAccessHelper =
      photoAccessHelper.getPhotoAccessHelper(this.context);
    const config: photoAccessHelper.PhotoCreationConfig = {
      title: title,
      fileNameExtension: extension,
      photoType: photoAccessHelper.PhotoType.IMAGE,
    };

    // 2026-11-28 (BUG-32): previously every save popped a
    // showAssetsCreationDialog confirm per image — batch exports meant N
    // taps. Now: request the WRITE_IMAGEVIDEO user_grant permission once
    // (system dialog), then createAsset writes directly with no per-image
    // dialog. Falls back to showAssetsCreationDialog only if the user
    // denies the permission.
    const writeImageVideo: Permissions = 'ohos.permission.WRITE_IMAGEVIDEO';
    this.ensureRuntimePermission(writeImageVideo).then((granted: boolean): void => {
      if (granted) {
        // Direct write: no per-image dialog, no user interaction needed.
        helper.createAsset(photoAccessHelper.PhotoType.IMAGE, extension, { title: title })
          .then((assetUri: string): void => {
            try {
              const srcFile: fs.File = fs.openSync(sandboxPath, fs.OpenMode.READ_ONLY);
              const desFile: fs.File = fs.openSync(assetUri, fs.OpenMode.READ_WRITE);
              fs.copyFileSync(srcFile.fd, desFile.fd);
              fs.closeSync(srcFile);
              fs.closeSync(desFile);
              console.info(`[RapidRAW] save_to_gallery ok (createAsset): ${assetUri}`);
              this.unlinkQuietly(sandboxPath);
              resolveSaveToGallery(requestId, true, null);
            } catch (e) {
              console.error(`[RapidRAW] save_to_gallery createAsset copy failed: ${JSON.stringify(e)}`);
              this.unlinkQuietly(sandboxPath);
              resolveSaveToGallery(requestId, false, `copy failed: ${JSON.stringify(e)}`);
            }
          })
          .catch((e: BusinessError): void => {
            // createAsset failed (e.g. permission revoked after grant, or a
            // system-level restriction) — fall back to the dialog path.
            console.warn(`[RapidRAW] save_to_gallery createAsset failed (${JSON.stringify(e)}), falling back to dialog`);
            this.saveToGalleryViaDialog(sandboxPath, helper, config, requestId, resolveSaveToGallery);
          });
      } else {
        // User denied WRITE_IMAGEVIDEO — the dialog path is the only way
        // to save to the gallery with explicit consent.
        console.warn('[RapidRAW] save_to_gallery: WRITE_IMAGEVIDEO denied, using dialog');
        this.saveToGalleryViaDialog(sandboxPath, helper, config, requestId, resolveSaveToGallery);
      }
    }).catch((e: BusinessError): void => {
      console.error(`[RapidRAW] save_to_gallery permission request threw: ${JSON.stringify(e)}`);
      this.saveToGalleryViaDialog(sandboxPath, helper, config, requestId, resolveSaveToGallery);
    });
  }

  /// Check and (if not yet granted) request the given user_grant
  /// permission via the system dialog. Resolves true when granted.
  private ensureRuntimePermission(permission: Permissions): Promise<boolean> {
    const atManager = abilityAccessCtrl.createAtManager();
    return atManager.checkAccessToken(this.context.applicationInfo.accessTokenId, permission)
      .then((status: number): Promise<boolean> => {
        if (status === 0) {
          // Already granted — no dialog needed.
          return Promise.resolve(true);
        }
        // Not granted yet — request it (the system shows the dialog).
        return atManager.requestPermissionsFromUser(this.context, [permission])
          .then((result: ESObject): boolean => {
            const statuses: Array<number> = result.grantStatus as Array<number>;
            return statuses.length > 0 && statuses[0] === 0;
          });
      });
  }

  /// Fallback: the per-image showAssetsCreationDialog (pre-2026-11-28
  /// behavior). Only reached when the user denies WRITE_IMAGEVIDEO or
  /// createAsset fails at runtime.
  private saveToGalleryViaDialog(
    sandboxPath: string,
    helper: photoAccessHelper.PhotoAccessHelper,
    config: photoAccessHelper.PhotoCreationConfig,
    requestId: number,
    resolveSaveToGallery: ESObject,
  ): void {
    const srcUri: string = fileUri.getUriFromPath(sandboxPath);
    helper.showAssetsCreationDialog([srcUri], [config]).then((uris: Array<string>) => {
      const first: string = uris.length > 0 ? uris[0] : '';
      if (first.startsWith('file://')) {
        try {
          const srcFile: fs.File = fs.openSync(sandboxPath, fs.OpenMode.READ_ONLY);
          const desFile: fs.File = fs.openSync(first, fs.OpenMode.READ_WRITE | fs.OpenMode.CREATE);
          fs.copyFileSync(srcFile.fd, desFile.fd);
          fs.closeSync(srcFile);
          fs.closeSync(desFile);
          console.info(`[RapidRAW] save_to_gallery ok: ${first}`);
          this.unlinkQuietly(sandboxPath);
          resolveSaveToGallery(requestId, true, null);
        } catch (e) {
          console.error(`[RapidRAW] save_to_gallery copy failed: ${JSON.stringify(e)}`);
          this.unlinkQuietly(sandboxPath);
          resolveSaveToGallery(requestId, false, `copy failed: ${JSON.stringify(e)}`);
        }
      } else if (first !== '') {
        // A non-URI entry is a documented batch-creation error code.
        console.error(`[RapidRAW] save_to_gallery failed: ${first}`);
        this.unlinkQuietly(sandboxPath);
        resolveSaveToGallery(requestId, false, `gallery save error ${first}`);
      } else {
        // An empty list means the user rejected the save dialog.
        console.info('[RapidRAW] save_to_gallery rejected by user');
        this.unlinkQuietly(sandboxPath);
        resolveSaveToGallery(requestId, false, 'user rejected the gallery save');
      }
    }).catch((e: BusinessError) => {
      console.error(`[RapidRAW] save_to_gallery dialog failed: ${JSON.stringify(e)}`);
      this.unlinkQuietly(sandboxPath);
      resolveSaveToGallery(requestId, false, `showAssetsCreationDialog failed: ${JSON.stringify(e)}`);
    });
  }

  private saveFileAs(req: Record<string, Object>, requestId: number, resolveSaveFileAs: ESObject): void {
    const fileName: string = req['fileName'] as string;
    const tempPath: string | null = req['tempPath'] === undefined ? null : req['tempPath'] as string;
    const options: picker.DocumentSaveOptions = new picker.DocumentSaveOptions();
    options.newFileNames = [fileName];
    const documentPicker = new picker.DocumentViewPicker(this.context);
    documentPicker.save(options).then((uris: Array<string>) => {
      if (!uris || uris.length === 0) {
        console.info('[RapidRAW] save_file_as cancelled');
        resolveSaveFileAs(requestId, null, null);
        return;
      }
      const targetPath: string = new fileUri.FileUri(uris[0]).path || uris[0];
      console.info(`[RapidRAW] save_file_as target: ${targetPath}`);
      if (tempPath !== null) {
        try {
          fs.copyFileSync(tempPath, targetPath);
          this.unlinkQuietly(tempPath);
        } catch (e) {
          console.error(`[RapidRAW] save_file_as copy failed: ${JSON.stringify(e)}`);
          resolveSaveFileAs(requestId, null, `copy failed: ${JSON.stringify(e)}`);
          return;
        }
      }
      resolveSaveFileAs(requestId, targetPath, null);
    }).catch((e: BusinessError) => {
      console.error(`[RapidRAW] save picker failed: ${JSON.stringify(e)}`);
      resolveSaveFileAs(requestId, null, JSON.stringify(e));
    });
  }

  // Convert the picker's temporary folder grant into a persistent one
  // (fileShare.persistPermission) and record the URI for later launches.
  // The capability used to be 2in1-only (docs 6.14), so this now attempts it
  // on every form factor: if the platform accepts it the grant survives
  // restarts, and if it does not the temporary grant still covers the session
  // and the frontend's per-launch re-grant flow stays the fallback.
  private persistFolderGrant(uri: string, path: string): void {
    // READ|WRITE: .rrdata sidecars are written next to the originals in
    // the library folder (parse_virtual_path in file_management.rs).
    const policy: fileShare.PolicyInfo = {
      uri: uri,
      operationMode: fileShare.OperationMode.READ_MODE | fileShare.OperationMode.WRITE_MODE,
    };
    try {
      fileShare.persistPermission([policy]).then(() => {
        console.info(`[RapidRAW] persistPermission ok for ${uri} (deviceType=${deviceInfo.deviceType})`);
        this.recordFolderGrant(uri, path);
      }).catch((e: BusinessError) => {
        // Non-fatal: expected 201 when the profile lacks the
        // FILE_ACCESS_PERSIST ACL and on form factors without the
        // persistence syscap (docs 6.14).
        console.error(`[RapidRAW] persistPermission failed (deviceType=${deviceInfo.deviceType}): ${JSON.stringify(e)}`);
      });
    } catch (e) {
      // Some form factors do not export the API at all; a synchronous throw
      // here must not break the folder pick.
      console.error(`[RapidRAW] persistPermission unavailable (deviceType=${deviceInfo.deviceType}): ${JSON.stringify(e)}`);
    }
  }

  private recordFolderGrant(uri: string, path: string): void {
    try {
      const grantsFile: string = `${this.context.filesDir}/ohos_grants.json`;
      let grants: Array<Record<string, string>> = [];
      try {
        grants = JSON.parse(fs.readTextSync(grantsFile)) as Array<Record<string, string>>;
      } catch (e) {
        grants = [];
      }
      if (!grants.some((g: Record<string, string>) => g['uri'] === uri)) {
        const entry: Record<string, string> = { 'uri': uri, 'path': path };
        grants.push(entry);
        const file: fs.File = fs.openSync(grantsFile,
          fs.OpenMode.READ_WRITE | fs.OpenMode.CREATE | fs.OpenMode.TRUNC);
        fs.writeSync(file.fd, JSON.stringify(grants));
        fs.closeSync(file);
      }
    } catch (e) {
      console.error(`[RapidRAW] recordFolderGrant failed: ${JSON.stringify(e)}`);
    }
  }

  // Persistent grants are not loaded into the kernel on app start; call
  // activatePermission for every recorded folder URI before the webview
  // reads the library. Per-URI isolation: one stale entry must not block
  // the rest.
  private async activatePersistedGrants(): Promise<void> {
    try {
      const grantsFile: string = `${this.context.filesDir}/ohos_grants.json`;
      let grants: Array<Record<string, string>> = [];
      try {
        grants = JSON.parse(fs.readTextSync(grantsFile)) as Array<Record<string, string>>;
      } catch (e) {
        return;
      }
      let activated: number = 0;
      for (const g of grants) {
        const policy: fileShare.PolicyInfo = {
          uri: g['uri'],
          operationMode: fileShare.OperationMode.READ_MODE | fileShare.OperationMode.WRITE_MODE,
        };
        try {
          await fileShare.activatePermission([policy]);
          activated++;
        } catch (e) {
          console.error(`[RapidRAW] activate grant failed for ${g['uri']}: ${JSON.stringify(e)}`);
        }
      }
      if (activated > 0) {
        console.info(`[RapidRAW] activated ${activated} persisted folder grant(s)`);
      }
    } catch (e) {
      console.error(`[RapidRAW] activatePersistedGrants failed: ${JSON.stringify(e)}`);
    }
  }

  // Withdraw recorded folder grants: deactivate the platform permission and
  // forget the URI. The record matters as much as the deactivation — as long as
  // an entry survives, the next launch's activatePersistedGrants() brings the
  // permission back, which is exactly why "remove the folder" (and the privacy
  // revoke, whose confirmation promises that file access must be re-authorized)
  // used to change nothing at the OS level (docs 6.20).
  //
  // `identifiers === null` withdraws every recorded grant. Entries are matched
  // by picker URI *or* resolved path: the frontend knows the path
  // (`/storage/Users/…`), the record is keyed by the picker URI (`file://docs/…`).
  // A failed deactivation still drops the record, so a later launch cannot
  // resurrect a grant the user asked to revoke.
  private async revokeFolderGrants(identifiers: Array<string> | null): Promise<Record<string, Object>> {
    const grantsFile: string = `${this.context.filesDir}/ohos_grants.json`;
    let grants: Array<Record<string, string>> = [];
    try {
      grants = JSON.parse(fs.readTextSync(grantsFile)) as Array<Record<string, string>>;
    } catch (e) {
      grants = [];
    }

    const wanted = (g: Record<string, string>): boolean => {
      if (identifiers === null) {
        return true;
      }
      const uri: string = g['uri'] ?? '';
      const path: string = g['path'] ?? '';
      return identifiers.includes(uri) || identifiers.includes(path);
    };

    const targets: Array<Record<string, string>> = grants.filter(wanted);
    const remaining: Array<Record<string, string>> = grants.filter((g: Record<string, string>) => !wanted(g));
    const revoked: Array<string> = [];
    const errors: Array<string> = [];

    for (const g of targets) {
      const policy: fileShare.PolicyInfo = {
        uri: g['uri'],
        operationMode: fileShare.OperationMode.READ_MODE | fileShare.OperationMode.WRITE_MODE,
      };
      try {
        await fileShare.deactivatePermission([policy]);
        revoked.push(g['uri']);
      } catch (e) {
        errors.push(`deactivate failed for ${g['uri']}: ${JSON.stringify(e)}`);
      }
    }

    try {
      const file: fs.File = fs.openSync(grantsFile,
        fs.OpenMode.READ_WRITE | fs.OpenMode.CREATE | fs.OpenMode.TRUNC);
      fs.writeSync(file.fd, JSON.stringify(remaining));
      fs.closeSync(file);
    } catch (e) {
      errors.push(`rewriting ohos_grants.json failed: ${JSON.stringify(e)}`);
    }

    const errorText: string = errors.length > 0 ? errors.join('; ') : '';
    console.info(`[RapidRAW] revoke_folder_grants: ${revoked.length} revoked, ${errors.length} error(s)`);
    return {
      'ok': errors.length === 0,
      'revoked': revoked,
      'errors': errors,
      'error': errorText,
    };
  }

  private handleRevokeFolderGrants(req: Record<string, Object>, requestId: number,
    resolveRevokeGrants: ESObject): void {
    const raw: Object | undefined = req['uris'];
    const identifiers: Array<string> | null = Array.isArray(raw) ? (raw as Array<string>) : null;
    this.revokeFolderGrants(identifiers).then((payload: Record<string, Object>) => {
      resolveRevokeGrants(requestId, JSON.stringify(payload));
    }).catch((error: BusinessError) => {
      console.error(`[RapidRAW] revoke_folder_grants failed: ${JSON.stringify(error)}`);
      resolveRevokeGrants(requestId, JSON.stringify({
        'ok': false,
        'error': `revoke_folder_grants failed: ${JSON.stringify(error)}`,
      }));
    });
  }

  private unlinkQuietly(path: string): void {
    try {
      fs.unlinkSync(path);
    } catch (e) {
      // The Rust side removes the temp file as a backstop.
      console.info(`[RapidRAW] temp unlink skipped: ${JSON.stringify(e)}`);
    }
  }
}

