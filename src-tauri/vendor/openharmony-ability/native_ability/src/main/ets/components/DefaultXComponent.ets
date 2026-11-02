import { NodeContent } from "@kit.ArkUI";
import {
  ArkHelper,
  WebViewInitData as NativeWebViewInitData,
  WindowAvoidAreaInfo,
} from "../ability/type";
import { exit, objectAssign } from "../helper";
import { Loadable } from "../helper/loadable";
import { requestPermission } from "../helper/permission";
import common from "@ohos.app.ability.common";
import window from "@ohos.window";
import {
  EmbeddedWebviewManager,
  RustWebviewNodeController,
  WebviewStyle,
  WebviewInitData,
} from "../webview/DefaultWebview";

export const RouteName = "NativeAbility";

// Theme base color for the webview surface. tauri.conf.json keeps
// `transparent: true` (the desktop frameless window needs it), which used to
// force the ArkWeb component background to Color.Transparent here; pixels the
// page has not painted yet then showed the WINDOW surface - black on OHOS -
// because a transparent ArkWeb reveals the window, not the ArkUI siblings
// behind it (docs HARMONYOS_PORTING.md 6.25). Paint the theme base color
// instead. EntryAbility seeds AppStorage('ohosBaseDark') from the system
// color mode before the page loads and on every onConfigurationUpdate, and
// the @StorageProp below follows it live.
const ohosThemeBackgroundColor = (): string => {
  return AppStorage.get<boolean>('ohosBaseDark') === false ? '#ffffff' : '#000000';
};

@Component
export struct DefaultXComponent {
  moduleName: string = "";
  private rootSlot = new NodeContent();
  private webviewController = new RustWebviewNodeController(this.getUIContext());
  private embeddedWebviewManager = new EmbeddedWebviewManager(this.getUIContext());
  private nativeModule: ESObject;
  // Webview created last through createWebview; used to push live theme
  // changes into its background style.
  private mainWebTag: string | null = null;
  @StorageProp("ohosBaseDark") @Watch("onBaseDarkChanged") baseDark: boolean = true;
  private helper: ArkHelper = {
    exit,
    requestPermission: async (permission: string | string[]): Promise<number | number[]> => {
      const context = this.getUIContext().getHostContext() as common.UIAbilityContext;
      return await requestPermission(context, permission);
    },
    getWindowAvoidArea: (type: number): WindowAvoidAreaInfo | undefined => {
      try {
        const context = this.getUIContext().getHostContext() as common.UIAbilityContext;
        const win = context.windowStage.getMainWindowSync();
        const avoidType = type as window.AvoidAreaType;
        return {
          type: avoidType,
          area: win.getWindowAvoidArea(avoidType),
        } as WindowAvoidAreaInfo;
      } catch (_) {
        return undefined;
      }
    },
    createWebview: (data: NativeWebViewInitData) => {
      const initScripts: ScriptItem[] = (data?.initializationScripts || []).map((i) => {
        return {
          script: i,
          scriptRules: ["*"],
        } as ScriptItem;
      });
      const init: WebviewInitData = {
        webTag: data?.id,
        url: data?.url,
        html: data?.html,
        headers: data?.headers,
        style: (data.style || {}) as WebviewStyle,
        javascriptEnable: data?.javascriptEnable ?? true,
        userAgent: data?.userAgent,
        devtools: data?.devtools,
        autoplay: data?.autoplay,
        initializationScripts: initScripts,
        onDragAndDrop: data?.onDragAndDrop,
        onDownloadStart: data?.onDownloadStart,
        onDownloadEnd: data?.onDownloadEnd,
        onNavigationRequest: data?.onNavigationRequest,
        onTitleChange: data?.onTitleChange,
      } as WebviewInitData;

      init.style = init.style || {};

      if (data?.transparent && !init.style.backgroundColor) {
        init.style.backgroundColor = ohosThemeBackgroundColor();
      }
      const ret = this.webviewController.addWebview(init);
      this.mainWebTag = ret.webTag;
      const applyStyle = (style: WebviewStyle) => {
        objectAssign(init.style as Record<string, ESObject>, style);
        this.webviewController.updateWebviewStyle(ret.webTag, init.style as WebviewStyle);
      };

      ret.controller.setBackgroundColor = (color: string) => {
        applyStyle({ backgroundColor: color });
      };

      ret.controller.setVisible = (visible: boolean) => {
        applyStyle({ visible });
      };

      ret.controller.dispose = () => {
        this.webviewController.removeWebview(ret.webTag);
      };

      return ret.controller;
    },
    createEmbeddedWebview: (data: NativeWebViewInitData) => {
      const initScripts: ScriptItem[] = (data?.initializationScripts || []).map((i) => {
        return {
          script: i,
          scriptRules: ["*"],
        } as ScriptItem;
      });
      const init: WebviewInitData = {
        webTag: data?.id,
        url: data?.url,
        html: data?.html,
        headers: data?.headers,
        style: (data.style || {}) as WebviewStyle,
        javascriptEnable: data?.javascriptEnable ?? true,
        userAgent: data?.userAgent,
        devtools: data?.devtools,
        autoplay: data?.autoplay,
        initializationScripts: initScripts,
        onDragAndDrop: data?.onDragAndDrop,
        onDownloadStart: data?.onDownloadStart,
        onDownloadEnd: data?.onDownloadEnd,
        onNavigationRequest: data?.onNavigationRequest,
        onTitleChange: data?.onTitleChange,
      } as WebviewInitData;

      init.style = init.style || {};

      if (data?.transparent && !init.style.backgroundColor) {
        init.style.backgroundColor = ohosThemeBackgroundColor();
      }

      const ret = this.embeddedWebviewManager.createWebview(init);
      const applyStyle = (style: WebviewStyle) => {
        objectAssign(init.style as Record<string, ESObject>, style);
        this.embeddedWebviewManager.updateWebviewStyle(ret.webTag, init.style as WebviewStyle);
      };

      ret.controller.setBackgroundColor = (color: string) => {
        applyStyle({ backgroundColor: color });
      };

      ret.controller.setVisible = (visible: boolean) => {
        applyStyle({ visible });
      };

      ret.controller.dispose = () => {
        this.embeddedWebviewManager.removeWebview(ret.webTag);
      };

      return ret;
    },
  };
  @StorageProp("loadMode") loadMode: "async" | "sync" = "async";

  // Live theme change: repaint the already-created webview's background
  // (docs 6.25). Embedded webviews only get the creation-time color.
  onBaseDarkChanged(): void {
    const tag = this.mainWebTag;
    if (tag === null) {
      return;
    }
    this.webviewController.updateWebviewStyle(tag, {
      backgroundColor: this.baseDark ? '#000000' : '#ffffff',
    } as WebviewStyle);
  }

  async aboutToAppear(): Promise<void> {
    const moduleName = this.moduleName.trim();
    if (!moduleName) {
      throw new Error("DefaultXComponent.moduleName is required");
    }
    this.nativeModule = await Loadable.load(moduleName, this.loadMode);
    this.nativeModule.render(this.helper, this.rootSlot);
  }

  build() {
    Stack() {
      ContentSlot(this.rootSlot)
      Row() {
        Column() {
          NodeContainer(this.webviewController)
            .height("100%")
            .width("100%")
        }.width("100%")
      }
      .height("100%")
      .hitTestBehavior(HitTestMode.Transparent)
    };
  }
}
