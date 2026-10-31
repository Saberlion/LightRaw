import { RustAbility } from '@ohos-rs/ability'
import Want from '@ohos.app.ability.Want'
import { AbilityConstant, Configuration, ConfigurationConstant, common } from '@kit.AbilityKit';
import { BusinessError, commonEventManager } from '@kit.BasicServicesKit';
import { KeyboardAvoidMode } from '@ohos.arkui.UIContext';
import window from '@ohos.window';
import picker from '@ohos.file.picker';
import fileUri from '@ohos.file.fileuri';
import fs from '@ohos.file.fs';
import { fileShare } from '@kit.CoreFileKit';
import deviceInfo from '@ohos.deviceInfo';
import { photoAccessHelper } from '@kit.MediaLibraryKit';
import { privacyManager } from '@kit.AppGalleryKit';

export default class EntryAbility extends RustAbility {
  // change to dynamic library name
  public moduleName: string = "rapidraw_lib"
  // Load our own pages/Index instead of the crate's MainPage: the custom
  // page adds the page-level onBackPress bridge (docs 6.5). The webview is
  // still created by DefaultXComponent inside that page.
  public defaultPage: boolean = false;
  public mode: 'xcomponent' | 'webview' = 'webview'
  private rapidrawModule: ESObject | null = null;
  // PRIVACY_STATE_CHANGED subscription (AppGallery standardized privacy
  // hosting); kept so a future teardown path can unsubscribe.
  private privacySubscriber: commonEventManager.CommonEventSubscriber | null = null;

  async onCreate(want: Want, launchParam: AbilityConstant.LaunchParam): Promise<void> {
    super.onCreate(want, launchParam);
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

  async onWindowStageCreate(windowStage: window.WindowStage): Promise<void> {
    // NOTE: local name must NOT be `window` — it would shadow the module
    // import and break enum access like window.AvoidAreaType (docs 6.8).
    const win = windowStage.getMainWindowSync();
    await win.setWindowLayoutFullScreen(false);
    // Safe-area evidence: with layoutFullScreen(false) the window manager
    // reserves the system bars already; log the remaining avoid rect.
    try {
      const avoid = win.getWindowAvoidArea(window.AvoidAreaType.TYPE_SYSTEM);
      console.info(`[RapidRAW] system avoid area: ${JSON.stringify(avoid)}`);
    } catch (e) {
      console.error(`[RapidRAW] getWindowAvoidArea failed: ${JSON.stringify(e)}`);
    }
    // RapidRAW's custom titlebar drives window controls through tauri
    // commands into ohos_integration.rs, which calls back into the JS
    // callback registered here (tao-ohos stubs the native window ops).
    // Fire-and-forget on purpose: awaiting lifecycle helpers here once
    // caused the ability to terminate right after startup (docs 6.4).
    this.registerRapidrawWindowController(win);
    super.onWindowStageCreate(windowStage);
    await windowStage.loadContent('pages/Index');
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

  // Machine-local post-init patch — see docs/HARMONYOS_PORTING.md 6.4.
  private async registerRapidrawWindowController(win: window.Window): Promise<void> {
    try {
      const libName: string = 'librapidraw_lib.so';
      const mod: ESObject = await import(libName);
      const register: ESObject =
        mod.registerOhosWindowController ?? mod.default?.registerOhosWindowController;
      if (typeof register !== 'function') {
        console.error('[RapidRAW] registerOhosWindowController not exported by librapidraw_lib.so');
        return;
      }
      const resolvePickFolder: ESObject =
        mod.resolveOhosPickFolder ?? mod.default?.resolveOhosPickFolder;
      const resolvePickFiles: ESObject =
        mod.resolveOhosPickFiles ?? mod.default?.resolveOhosPickFiles;
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
        console.error('[RapidRAW] bridge resolve exports missing from librapidraw_lib.so');
      }
      if (typeof resolvePrivacy !== 'function') {
        console.error('[RapidRAW] resolveOhosPrivacy missing from librapidraw_lib.so');
      }
      if (typeof resolveOpenUrl !== 'function') {
        console.error('[RapidRAW] resolveOhosOpenUrl missing from librapidraw_lib.so');
      }
      if (typeof resolveRevokeGrants !== 'function') {
        console.error('[RapidRAW] resolveOhosRevokeGrants missing from librapidraw_lib.so');
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
            this.handleBridgeRequest(req, resolvePickFolder, resolvePickFiles, resolveSaveToGallery,
              resolveSaveFileAs, resolvePrivacy, resolveOpenUrl, resolveRevokeGrants);
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
      // Subscribe only now that the module handle exists: the consent
      // subscription pushes through notifyOhosPrivacyChanged below.
      this.subscribePrivacyEvents();
      console.info('[RapidRAW] OHOS window controller registered');
    } catch (e) {
      console.error(`[RapidRAW] window-controller registration failed: ${JSON.stringify(e)}`);
    }
  }

  onConfigurationUpdate(newConfig: Configuration): void {
    super.onConfigurationUpdate(newConfig);
    this.pushColorMode(newConfig.colorMode);
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

  // Forward the device form factor into the Rust bridge. The frontend uses it
  // to choose between the desktop (2in1) and touch-first tablet layouts; the
  // module import resolves after the page may already have loaded, so the Rust
  // side also emits 'ohos-device-type' for the frontend to follow (docs 6.14).
  private pushDeviceType(): void {
    const notify: ESObject =
      this.rapidrawModule?.notifyOhosDeviceType ?? this.rapidrawModule?.default?.notifyOhosDeviceType;
    if (typeof notify !== 'function') {
      console.error('[RapidRAW] notifyOhosDeviceType not exported by librapidraw_lib.so');
      return;
    }
    notify(deviceInfo.deviceType);
  }

  // Every handler MUST resolve its request exactly once — the Rust side parks
  // a oneshot channel per request id and has no timeout.
  private handleBridgeRequest(
    req: Record<string, Object>,
    resolvePickFolder: ESObject,
    resolvePickFiles: ESObject,
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
        this.pickGalleryImages(req, requestId, resolvePickFiles);
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

  private pickFolder(requestId: number, resolvePickFolder: ESObject): void {
    const options: picker.DocumentSelectOptions = new picker.DocumentSelectOptions();
    options.selectMode = picker.DocumentSelectMode.FOLDER;
    options.maxSelectNumber = 1;
    const documentPicker = new picker.DocumentViewPicker(this.context);
    documentPicker.select(options).then((uris: Array<string>) => {
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
    documentPicker.select(options).then((uris: Array<string>) => {
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
  private pickGalleryImages(req: Record<string, Object>, requestId: number, resolvePickGallery: ESObject): void {
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
    photoPicker.select(options).then((result: photoAccessHelper.PhotoSelectResult) => {
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
        if (wanted.size > 0 && !wanted.has(extension)) {
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
      try {
        try {
          fs.accessSync(stagingRoot);
        } catch (accessError) {
          fs.mkdirSync(stagingRoot, true);
        }
        try {
          fs.accessSync(stagingDir);
        } catch (accessError) {
          fs.mkdirSync(stagingDir, true);
        }
      } catch (e) {
        console.error(`[RapidRAW] pick_gallery_images staging dir failed: ${JSON.stringify(e)}`);
        resolvePickGallery(requestId, []);
        return;
      }

      const staged: Array<string> = [];
      const usedNames: Set<string> = new Set<string>();
      const startedAt: number = Date.now();
      for (let i = 0; i < pickedUris.length; i++) {
        // Keep the original display name so the import filename template
        // ({original_filename}) still produces the expected name. Duplicates
        // within one pick get an index suffix.
        let name: string = pickedNames[i];
        if (usedNames.has(name)) {
          const dot: number = name.lastIndexOf('.');
          const stem: string = dot > 0 ? name.substring(0, dot) : name;
          const extension: string = dot > 0 ? name.substring(dot) : '';
          name = `${stem}_${i}${extension}`;
        }
        usedNames.add(name);
        const target: string = `${stagingDir}/${name}`;
        try {
          const source: fs.File = fs.openSync(pickedUris[i], fs.OpenMode.READ_ONLY);
          try {
            fs.copyFileSync(source.fd, target);
          } finally {
            fs.closeSync(source);
          }
          staged.push(target);
        } catch (e) {
          console.error(`[RapidRAW] pick_gallery_images copy failed for ${pickedUris[i]}: ${JSON.stringify(e)}`);
        }
      }
      console.info(
        `[RapidRAW] pick_gallery_images staged ${staged.length}/${pickedUris.length} in ${Date.now() - startedAt}ms`,
      );
      resolvePickGallery(requestId, staged);
    }).catch((e: BusinessError) => {
      console.error(`[RapidRAW] gallery picker failed: ${JSON.stringify(e)}`);
      resolvePickGallery(requestId, []);
    });
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
    // Tauri's desktop path resolver yields a non-sandbox cache path on OHOS,
    // which the system save dialog cannot authorize (14000011). Stage the
    // bytes into this ability's cacheDir first: URIs derived from it are the
    // canonical sandbox form showAssetsCreationDialog accepts.
    const sandboxDir: string = `${this.context.cacheDir}/export_bridge`;
    const stagedName: string = tempPath.substring(tempPath.lastIndexOf('/') + 1);
    const sandboxPath: string = `${sandboxDir}/${stagedName}`;
    try {
      // OHOS mkdirSync is not idempotent: an existing directory throws
      // 13900015 ("File exists") even with recursion enabled, so probe
      // for existence first.
      try {
        fs.accessSync(sandboxDir);
      } catch (accessError) {
        fs.mkdirSync(sandboxDir, true);
      }
      fs.copyFileSync(tempPath, sandboxPath);
    } catch (e) {
      console.error(`[RapidRAW] save_to_gallery staging failed: ${JSON.stringify(e)}`);
      resolveSaveToGallery(requestId, false, `staging failed: ${JSON.stringify(e)}`);
      return;
    }
    const srcUri: string = fileUri.getUriFromPath(sandboxPath);
    const helper: photoAccessHelper.PhotoAccessHelper =
      photoAccessHelper.getPhotoAccessHelper(this.context);
    const config: photoAccessHelper.PhotoCreationConfig = {
      title: title,
      fileNameExtension: extension,
      photoType: photoAccessHelper.PhotoType.IMAGE,
    };
    // createAsset needs the restricted WRITE_IMAGEVIDEO permission (fails
    // with error 201); this dialog saves with explicit user consent instead.
    // On consent the system creates the asset and returns a URI with a
    // permanent write grant, but the app must copy the bytes itself.
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
