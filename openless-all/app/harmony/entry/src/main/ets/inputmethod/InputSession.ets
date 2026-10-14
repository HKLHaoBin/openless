import { inputMethodEngine, InputMethodExtensionContext } from '@kit.IMEKit';
import { BusinessError } from '@kit.BasicServicesKit';
import { initializeRime, rimeRequest, RimeState, testRime } from './RimeClient';

class InputPanelState {
  connected: boolean = false;
  status: string = '等待输入框';
  enterLabel: string = '换行';
  chinese: boolean = true;
  preedit: string = '';
  candidates: string[] = [];
  rimeReady: boolean = false;
  page: number = 0;
  hasNext: boolean = false;
  password: boolean = false;
}

export const imeStorage: LocalStorage = new LocalStorage(new InputPanelState());

// Only the system-provided client may edit the target app. Never retain a
// queued operation across inputStop/inputStart or inspect another app's text.
class InputSession {
  private client?: inputMethodEngine.InputClient;
  private keyboard?: inputMethodEngine.KeyboardController;
  private context?: InputMethodExtensionContext;
  private action: number = inputMethodEngine.ENTER_KEY_TYPE_UNSPECIFIED;
  private queue: Promise<void> = Promise.resolve();
  private generation: number = 0;
  private rimeSession: string = '';
  private encoding: string = '';
  private candidateRevision: number = 0;

  attach(context: InputMethodExtensionContext, keyboard: inputMethodEngine.KeyboardController,
    client: inputMethodEngine.InputClient): void {
    if (this.client) this.detach();
    this.generation++;
    const generation = this.generation;
    this.context = context;
    this.keyboard = keyboard;
    this.client = client;
    this.action = inputMethodEngine.ENTER_KEY_TYPE_UNSPECIFIED;
    imeStorage.setOrCreate('connected', true);
    imeStorage.setOrCreate('status', '可输入 · 语音功能接线中');
    imeStorage.setOrCreate('enterLabel', '换行');
    imeStorage.setOrCreate('rimeReady', false);
    this.resetCandidates();
    client.getEditorAttribute().then((attribute: inputMethodEngine.EditorAttribute) => {
      if (this.client !== client || generation !== this.generation) return;
      const password = [inputMethodEngine.PATTERN_PASSWORD, inputMethodEngine.PATTERN_PASSWORD_NUMBER,
        inputMethodEngine.PATTERN_PASSWORD_SCREEN_LOCK, inputMethodEngine.PATTERN_NEW_PASSWORD].includes(attribute.inputPattern);
      imeStorage.setOrCreate('chinese', !password);
      imeStorage.setOrCreate('password', password);
      if (!password) {
        this.queue = this.queue.then(async (): Promise<void> => {
          await initializeRime(context.filesDir, context.resourceManager);
          await testRime(context.filesDir, context.resourceManager);
          const state = await rimeRequest('create', '', 'private');
          if (generation !== this.generation) { await rimeRequest('destroy', state.session); return; }
          this.rimeSession = state.session;
          imeStorage.setOrCreate('rimeReady', true);
          this.publish(state);
        }).catch(() => { if (generation === this.generation) imeStorage.setOrCreate('status', '中文引擎加载失败，可切换英文'); });
      }
      this.action = attribute.enterKeyType;
      let label = '换行';
      if (this.action === inputMethodEngine.ENTER_KEY_TYPE_SEND) label = '发送';
      else if (this.action === inputMethodEngine.ENTER_KEY_TYPE_SEARCH) label = '搜索';
      else if (this.action === inputMethodEngine.ENTER_KEY_TYPE_DONE) label = '完成';
      else if (this.action === inputMethodEngine.ENTER_KEY_TYPE_NEXT) label = '下一项';
      else if (this.action === inputMethodEngine.ENTER_KEY_TYPE_GO) label = '前往';
      imeStorage.setOrCreate('enterLabel', label);
    }).catch(() => {});
  }

  detach(): void {
    this.generation++;
    this.client = undefined;
    this.keyboard = undefined;
    this.context = undefined;
    const session = this.rimeSession;
    this.rimeSession = '';
    if (session) this.queue = this.queue.then(async (): Promise<void> => { await rimeRequest('destroy', session); }).catch(() => {});
    this.resetCandidates();
    imeStorage.setOrCreate('connected', false);
    imeStorage.setOrCreate('status', '等待输入框');
  }

  private perform(kind: string, operation: (client: inputMethodEngine.InputClient) => Promise<boolean>): void {
    const client = this.client;
    const generation = this.generation;
    if (!client) return;
    const task = this.queue.then(async (): Promise<void> => {
      if (this.client !== client || this.generation !== generation) return;
      try {
        const ok = await operation(client);
        console.info('[OpenLess IME] ' + kind + ' result=' + ok);
        if (this.client === client && this.generation === generation) {
          imeStorage.setOrCreate('status', ok ? '可输入 · 语音功能接线中' : '输入未成功，请重新选择输入框');
        }
      } catch (error) {
        if (this.client === client && this.generation === generation) imeStorage.setOrCreate('status', '输入未成功，请重新选择输入框');
        console.error('[OpenLess IME] ' + kind + ' failed code=' + (error as BusinessError).code);
      }
    });
    this.queue = task.catch(() => {});
  }

  insert(text: string): void {
    if (imeStorage.get<boolean>('chinese') && /^[a-z]$/i.test(text)) {
      this.rimeOperation('key', text.toLowerCase().charCodeAt(0).toString());
      return;
    }
    if (imeStorage.get<boolean>('chinese')) { this.rimeOperation('literal', text); return; }
    if (text.length) this.perform('insert', (client: inputMethodEngine.InputClient) => client.insertText(text));
  }

  backspace(): void {
    // Check encoding inside the serial queue; fast typing may still be pending.
    this.rimeOperation('backspace', '');
  }

  private resetCandidates(): void {
    this.encoding = '';
    imeStorage.setOrCreate('preedit', '');
    imeStorage.setOrCreate('candidates', [] as string[]);
  }

  private publish(state: RimeState): void {
    this.candidateRevision = state.revision;
    this.encoding = state.input;
    imeStorage.setOrCreate('preedit', state.preedit || state.input);
    imeStorage.setOrCreate('candidates', state.candidates);
    imeStorage.setOrCreate('page', state.page);
    imeStorage.setOrCreate('hasNext', state.hasNext);
    imeStorage.setOrCreate('status', 'Rime 拼音 · 语音功能接线中');
  }

  private rimeOperation(operation: string, argument: string): void {
    const generation = this.generation;
    const client = this.client;
    if (!client) return;
    this.queue = this.queue.then(async (): Promise<void> => {
      if (generation !== this.generation || client !== this.client) return;
      if (operation === 'backspace' && (!this.rimeSession || !this.encoding)) {
        const ok = await client.deleteForward(1);
        console.info('[OpenLess IME] backspace result=' + ok);
        return;
      }
      if (operation === 'space' && !this.encoding) { await client.insertText(' '); return; }
      if (!this.rimeSession) throw new Error('中文引擎未就绪');
      const key = operation === 'backspace' ? '65288' : operation === 'space' ? '32' : argument;
      let state = await rimeRequest(operation === 'backspace' || operation === 'space' ? 'key' : operation === 'literal' ? 'confirm' : operation, this.rimeSession, key);
      if (generation !== this.generation || client !== this.client) return;
      if (state.commit) {
        if (!await client.insertText(state.commit)) throw new Error('文字写入失败，请重新确认');
        if (generation !== this.generation) return;
        state = await rimeRequest('ack', this.rimeSession);
      }
      if (operation === 'literal' && argument) await client.insertText(argument);
      this.publish(state);
    }).catch((error: Error) => { if (generation === this.generation) imeStorage.setOrCreate('status', error.message); });
  }

  selectCandidate(index: number): void { this.rimeOperation('select', index.toString() + '@' + this.candidateRevision.toString()); }
  space(): void { this.rimeOperation('space', ''); }
  page(next: boolean): void { this.rimeOperation('key', next ? '65366' : '65365'); }
  toggleLanguage(): void {
    const chinese = !imeStorage.get<boolean>('chinese');
    imeStorage.setOrCreate('chinese', chinese);
    if (this.rimeSession) this.rimeOperation('clear', '');
  }

  enter(): void {
    if (this.encoding) { this.rimeOperation('key', '65293'); return; }
    const action = this.action;
    if (action === inputMethodEngine.ENTER_KEY_TYPE_UNSPECIFIED || action === inputMethodEngine.ENTER_KEY_TYPE_NEWLINE) {
      this.insert('\n');
    } else {
      this.perform('enter', (client: inputMethodEngine.InputClient) => client.sendKeyFunction(action));
    }
  }

  hide(): void {
    this.keyboard?.hide().catch(() => {});
  }

  settings(): void {
    this.context?.startAbility({ bundleName: 'openless.hm.com', abilityName: 'EntryAbility' })
      .catch(() => { imeStorage.setOrCreate('status', '无法打开设置'); });
  }
}

export const inputSession: InputSession = new InputSession();
