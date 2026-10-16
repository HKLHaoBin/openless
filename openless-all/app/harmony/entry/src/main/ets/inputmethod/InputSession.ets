import { inputMethodEngine, InputMethodExtensionContext } from '@kit.IMEKit';
import { BusinessError } from '@kit.BasicServicesKit';
import { initializeRime, rimeRequest, RimeState, testRime } from './RimeClient';
import { VoiceClient } from './VoiceClient';

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
  voicePhase: string = 'idle';
  voiceText: string = '';
  voiceLevel: number = 0;
  pendingCommit: boolean = false;
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
  private voice: VoiceClient = new VoiceClient();
  private voiceGeneration: number = 0;

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
    imeStorage.setOrCreate('status', '可输入');
    imeStorage.setOrCreate('enterLabel', '换行');
    imeStorage.setOrCreate('rimeReady', false);
    imeStorage.setOrCreate('password', true);
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
          // Normal fields learn locally; passwords never create a Rime session.
          const state = await rimeRequest('create', '', 'learning');
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
    this.cancelVoice();
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
          imeStorage.setOrCreate('status', ok ? '可输入' : '输入未成功，请重新选择输入框');
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
    imeStorage.setOrCreate('pendingCommit', false);
  }

  private publish(state: RimeState): void {
    this.candidateRevision = state.revision;
    this.encoding = state.input;
    imeStorage.setOrCreate('preedit', state.preedit || state.input);
    imeStorage.setOrCreate('candidates', state.candidates);
    imeStorage.setOrCreate('page', state.page);
    imeStorage.setOrCreate('hasNext', state.hasNext);
    imeStorage.setOrCreate('pendingCommit', !!state.commit);
    imeStorage.setOrCreate('status', 'Rime 拼音');
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
        this.publish(state);
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
    const action = this.action;
    const generation = this.generation;
    this.perform('enter', async (client: inputMethodEngine.InputClient): Promise<boolean> => {
      if (this.encoding && this.rimeSession) {
        const state = await rimeRequest('key', this.rimeSession, '65293');
        if (generation !== this.generation || client !== this.client) return false;
        this.publish(state);
        if (state.commit) {
          if (!await client.insertText(state.commit)) return false;
          if (generation !== this.generation || client !== this.client) return false;
          this.publish(await rimeRequest('ack', this.rimeSession));
        }
        return true;
      }
      return action === inputMethodEngine.ENTER_KEY_TYPE_UNSPECIFIED || action === inputMethodEngine.ENTER_KEY_TYPE_NEWLINE ?
        await client.insertText('\n') : await client.sendKeyFunction(action);
    });
  }

  retryCommit(): void { this.rimeOperation('state', ''); }

  async startVoice(): Promise<void> {
    const context = this.context;
    const client = this.client;
    if (!context || !client || imeStorage.get<boolean>('password') || imeStorage.get<string>('voicePhase') !== 'idle') return;
    if (imeStorage.get<boolean>('pendingCommit')) { imeStorage.setOrCreate('status', '请先重新确认待写入的候选词'); return; }
    const generation = this.generation;
    const voiceGeneration = ++this.voiceGeneration;
    imeStorage.setOrCreate('voicePhase', 'starting');
    imeStorage.setOrCreate('voiceText', '');
    imeStorage.setOrCreate('status', '正在准备麦克风');
    try {
      // Clear the composition in the same queue as normal keyboard operations.
      await this.queue;
      if (generation !== this.generation || voiceGeneration !== this.voiceGeneration) return;
      if (this.rimeSession) this.publish(await rimeRequest('clear', this.rimeSession));
      if (generation !== this.generation || voiceGeneration !== this.voiceGeneration) return;
      await this.voice.start(context, (level: number) => {
        if (generation === this.generation && voiceGeneration === this.voiceGeneration) imeStorage.setOrCreate('voiceLevel', level);
      }, (message: string) => {
        if (generation === this.generation && voiceGeneration === this.voiceGeneration) {
          imeStorage.setOrCreate('voicePhase', 'idle');
          imeStorage.setOrCreate('status', message);
        }
      });
      if (generation !== this.generation || voiceGeneration !== this.voiceGeneration) return;
      imeStorage.setOrCreate('voicePhase', 'recording');
      imeStorage.setOrCreate('status', '正在录音，点结束开始识别');
    } catch (error) {
      if (generation === this.generation && voiceGeneration === this.voiceGeneration) {
        imeStorage.setOrCreate('voicePhase', 'idle');
        imeStorage.setOrCreate('status', (error as Error).message);
      }
    }
  }

  async stopVoice(): Promise<void> {
    if (imeStorage.get<string>('voicePhase') !== 'recording') return;
    const generation = this.generation;
    const voiceGeneration = this.voiceGeneration;
    imeStorage.setOrCreate('voicePhase', 'processing');
    imeStorage.setOrCreate('voiceLevel', 0);
    imeStorage.setOrCreate('status', '正在识别');
    try {
      const result = await this.voice.stop();
      if (generation !== this.generation || voiceGeneration !== this.voiceGeneration) return;
      const text = result.polishedText || result.rawText;
      imeStorage.setOrCreate('voiceText', text);
      imeStorage.setOrCreate('voicePhase', text ? 'review' : 'idle');
      imeStorage.setOrCreate('status', text ? '确认后写入当前输入框' : '未识别到文字，请重新录音');
    } catch (error) {
      if (generation === this.generation && voiceGeneration === this.voiceGeneration) {
        imeStorage.setOrCreate('voicePhase', 'idle');
        imeStorage.setOrCreate('status', (error as Error).message);
      }
    }
  }

  confirmVoice(): void {
    if (imeStorage.get<string>('voicePhase') !== 'review') return;
    const text = imeStorage.get<string>('voiceText') || '';
    const generation = this.generation;
    const voiceGeneration = this.voiceGeneration;
    imeStorage.setOrCreate('voicePhase', 'inserting');
    this.perform('voice insert', async (client: inputMethodEngine.InputClient): Promise<boolean> => {
      let ok = false;
      try { ok = await client.insertText(text); return ok; } finally {
        if (generation === this.generation && voiceGeneration === this.voiceGeneration) {
          imeStorage.setOrCreate('voicePhase', ok ? 'idle' : 'review');
          if (ok) imeStorage.setOrCreate('voiceText', '');
        }
      }
    });
  }

  cancelVoice(): void {
    const phase = imeStorage.get<string>('voicePhase');
    this.voiceGeneration++;
    this.voice.cancel().catch(() => {});
    imeStorage.setOrCreate('voicePhase', 'idle');
    imeStorage.setOrCreate('voiceText', '');
    imeStorage.setOrCreate('voiceLevel', 0);
    if (phase !== 'idle') imeStorage.setOrCreate('status', '录音已取消');
  }

  hide(): void {
    this.cancelVoice();
    this.keyboard?.hide().catch(() => {});
  }

  settings(): void {
    this.cancelVoice();
    this.context?.startAbility({ bundleName: 'openless.hm.com', abilityName: 'EntryAbility' })
      .catch(() => { imeStorage.setOrCreate('status', '无法打开设置'); });
  }
}

export const inputSession: InputSession = new InputSession();
