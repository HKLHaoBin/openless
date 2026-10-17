import { InputMethodExtensionAbility, inputMethodEngine } from '@kit.IMEKit';
import { display } from '@kit.ArkUI';
import { imeStorage, inputSession } from './InputSession';

export default class OpenLessInputMethod extends InputMethodExtensionAbility {
  private engine: inputMethodEngine.InputMethodAbility = inputMethodEngine.getInputMethodAbility();
  private panel?: inputMethodEngine.Panel;
  private ready?: Promise<void>;
  private active: boolean = false;
  private visible: boolean = false;

  private onStart = (keyboard: inputMethodEngine.KeyboardController, client: inputMethodEngine.InputClient): void => {
    inputSession.attach(this.context, keyboard, client);
    console.info('[OpenLess IME] inputStart');
  };
  private onStop = (): void => {
    inputSession.detach();
    this.visible = false;
    this.panel?.hide().catch(() => {});
    console.info('[OpenLess IME] inputStop');
  };
  private onShow = (): void => {
    this.visible = true;
    this.ensurePanel().then(() => {
      if (this.active && this.visible) return this.panel?.show();
      return undefined;
    }).catch(() => { imeStorage.setOrCreate('status', '键盘加载失败'); });
  };
  private onHide = (): void => {
    inputSession.cancelVoice();
    this.visible = false;
    this.panel?.hide().catch(() => {});
  };

  onCreate(): void {
    this.active = true;
    this.engine.on('inputStart', this.onStart);
    this.engine.on('inputStop', this.onStop);
    this.engine.on('keyboardShow', this.onShow);
    this.engine.on('keyboardHide', this.onHide);
    console.info('[OpenLess IME] created');
  }

  private ensurePanel(): Promise<void> {
    if (this.ready) return this.ready;
    this.ready = this.createPanel().catch(() => {
      this.ready = undefined;
      console.error('[OpenLess IME] panel creation failed');
      throw new Error('panel creation failed');
    });
    return this.ready;
  }

  private async createPanel(): Promise<void> {
    const panel = await this.engine.createPanel(this.context, {
      type: inputMethodEngine.PanelType.SOFT_KEYBOARD,
      flag: inputMethodEngine.PanelFlag.FLG_FIXED
    });
    if (!this.active) {
      await this.engine.destroyPanel(panel);
      return;
    }
    this.panel = panel;
    try {
      const screen = display.getDefaultDisplaySync();
      await panel.resize(screen.width, Math.round(300 * screen.densityPixels));
      await panel.setUiContent('pages/InputMethodPanel', imeStorage);
      console.info('[OpenLess IME] panel ready');
    } catch (_) {
      this.panel = undefined;
      await this.engine.destroyPanel(panel);
      throw new Error('panel content failed');
    }
  }

  onDestroy(): void {
    this.active = false;
    this.visible = false;
    this.engine.off('inputStart', this.onStart);
    this.engine.off('inputStop', this.onStop);
    this.engine.off('keyboardShow', this.onShow);
    this.engine.off('keyboardHide', this.onHide);
    inputSession.detach();
    const panel = this.panel;
    this.panel = undefined;
    if (panel) this.engine.destroyPanel(panel).catch(() => {});
    console.info('[OpenLess IME] destroyed');
  }
}
