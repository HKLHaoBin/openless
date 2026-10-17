import { inputMethod } from '@kit.IMEKit';
import { imeStorage, inputSession } from '../inputmethod/InputSession';
import { panelLayout } from '../inputmethod/PanelLayout';

const BACKGROUND: Resource = $r('app.color.ime_panel_background');
const KEY: Resource = $r('app.color.ime_key_background');
const FUNCTION_KEY: Resource = $r('app.color.ime_function_background');
const INK: Resource = $r('app.color.ime_foreground');
const MUTED: Resource = $r('app.color.ime_secondary');
const ACCENT: Resource = $r('app.color.ime_accent');

@Entry(imeStorage)
@Component
struct InputMethodPanel {
  @LocalStorageLink('connected') connected: boolean = false;
  @LocalStorageLink('status') status: string = '等待输入框';
  @LocalStorageLink('enterLabel') enterLabel: string = '换行';
  @LocalStorageLink('chinese') chinese: boolean = true;
  @LocalStorageLink('preedit') preedit: string = '';
  @LocalStorageLink('candidates') candidates: string[] = [];
  @LocalStorageLink('cloudEnabled') cloudEnabled: boolean = false;
  @LocalStorageLink('rimeReady') rimeReady: boolean = false;
  @LocalStorageLink('page') page: number = 0;
  @LocalStorageLink('hasNext') hasNext: boolean = false;
  @LocalStorageLink('password') password: boolean = false;
  @LocalStorageLink('editorVerified') editorVerified: boolean = false;
  @LocalStorageLink('voicePhase') voicePhase: string = 'idle';
  @LocalStorageLink('voiceText') voiceText: string = '';
  @LocalStorageLink('voiceLevel') voiceLevel: number = 0;
  @LocalStorageLink('pendingCommit') pendingCommit: boolean = false;
  @LocalStorageLink('writeInFlight') writeInFlight: boolean = false;
  @LocalStorageLink('keyboardLayout') keyboardLayout: string = 'standard';
  @LocalStorageLink('keyHeight') keyHeight: number = 52;
  @LocalStorageLink('showNumberRow') showNumberRow: boolean = true;
  @State uppercase: boolean = false;
  @State capsLock: boolean = false;
  @State layer: number = 0;
  @State utility: number = 0;
  @State expanded: boolean = false;
  @State symbolPage: number = 0;

  private row(index: number): string[] {
    if (this.layer === 2) {
      if (this.symbolPage === 1) return [
        ['$', '€', '£', '¥', '℃', '°', '×', '÷', '√', 'π'],
        ['[', ']', '{', '}', '<', '>', '=', '_', '~'],
        ['…', '—', '、', '·', '|', '\\', '^']
      ][index];
      return this.chinese ? [
        ['，', '。', '？', '！', '：', '；', '、', '…', '—', '·'],
        ['（', '）', '【', '】', '《', '》', '“', '”', '￥'],
        ['@', '#', '%', '&', '*', '+', '/']
      ][index] : [
        [',', '.', '?', '!', ':', ';', "'", '"', '-', '_'],
        ['(', ')', '[', ']', '{', '}', '<', '>', '$'],
        ['@', '#', '%', '&', '*', '+', '/']
      ][index];
    }
    const letters: string[] = this.keyboardLayout === 'split' ?
      ['qwertyuiop', 'asdfgghjkl', 'zxcvvbnm'] : ['qwertyuiop', 'asdfghjkl', 'zxcvbnm'];
    return (!this.chinese && this.uppercase ? letters[index].toUpperCase() : letters[index]).split('');
  }

  private hint(index: number, position: number): string {
    if (this.layer !== 0) return '';
    const hints: string[] = this.chinese ? ['1234567890', '@#￥%&-+（）', "，。？！：；'"] :
      ['1234567890', '@#$%&-+()', ",.?!:;'"];
    if (this.showNumberRow && index === 0) return '';
    const duplicated = this.keyboardLayout === 'split' &&
      ((index === 1 && position >= 5) || (index === 2 && position >= 4));
    return hints[index].charAt(duplicated ? position - 1 : position);
  }

  private insert(text: string): void {
    inputSession.insert(text);
    if (this.uppercase && !this.capsLock) this.uppercase = false;
  }

  private language(): void {
    inputSession.toggleLanguage();
    this.uppercase = false;
    this.capsLock = false;
  }

  private shift(): void {
    if (this.layer === 2) { this.symbolPage = this.symbolPage === 0 ? 1 : 0; return; }
    if (this.chinese) inputSession.toggleLanguage();
    this.uppercase = !this.uppercase;
    this.capsLock = false;
  }

  private inputMethods(): void {
    inputMethod.getSetting().showOptionalInputMethods().catch(() => {
      this.status = '请在系统输入法菜单切换';
    });
  }

  private voice(): void {
    this.utility = 0;
    this.expanded = false;
    inputSession.startVoice();
  }

  @Builder
  toolbarButton(label: string, description: string, action: () => void) {
    Button(label, { type: ButtonType.Normal })
      .height(32).layoutWeight(1).fontSize(17).fontColor(MUTED)
      .backgroundColor(Color.Transparent).borderRadius(8).padding(0).focusable(false)
      .accessibilityText(description).onClick(() => { action(); })
  }

  @Builder
  keyboardToolbar() {
    Row({ space: 6 }) {
      this.toolbarButton('☰', '输入法工具', () => { this.utility = this.utility === 1 ? 0 : 1; })
      this.toolbarButton('▦', '键盘布局', () => { this.utility = this.utility === 3 ? 0 : 3; })
      this.toolbarButton('☺', '表情', () => { this.utility = this.utility === 2 ? 0 : 2; })
      Button('语音', { type: ButtonType.Normal }).height(32).layoutWeight(1.4).fontSize(14)
        .backgroundColor(Color.Transparent).fontColor(MUTED).focusable(false)
        .enabled(this.connected && this.editorVerified && !this.password && !this.pendingCommit && this.voicePhase === 'idle')
        .onClick(() => { this.voice(); })
      this.toolbarButton('⌄', '收起键盘', () => { inputSession.hide(); })
    }.width('100%').height(34)
  }

  @Builder
  pendingBar() {
    Row({ space: 8 }) {
      Button('已写入', { type: ButtonType.Normal }).height(32).layoutWeight(1).fontSize(14)
        .fontColor(ACCENT).backgroundColor(KEY).padding(0).focusable(false)
        .accessibilityText('文字已出现在输入框，确认并继续输入，不重复写入')
        .onClick(() => { inputSession.acknowledgePendingCommit(); })
      Button('重试写入', { type: ButtonType.Normal }).height(32).layoutWeight(1).fontSize(14)
        .fontColor(INK).backgroundColor(KEY).padding(0).focusable(false)
        .enabled(!this.writeInFlight)
        .onClick(() => { inputSession.retryCommit(); })
      Button('取消待提交', { type: ButtonType.Normal }).height(32).layoutWeight(1).fontSize(14)
        .fontColor(INK).backgroundColor(KEY).padding(0).focusable(false)
        .onClick(() => { inputSession.cancelPendingCommit(); })
    }.height(34).width('100%')
  }

  @Builder
  candidateBar() {
    Row({ space: 4 }) {
      Button('☰', { type: ButtonType.Normal }).width(38).height(32).padding(0).fontSize(20)
        .fontColor(MUTED).backgroundColor(Color.Transparent).focusable(false)
        .accessibilityText('输入法工具').onClick(() => { this.utility = this.utility === 1 ? 0 : 1; })
      Scroll() {
        Row({ space: 2 }) {
          ForEach(this.candidates, (candidate: string, index: number) => {
            Button(candidate, { type: ButtonType.Normal }).height(32).fontSize(21)
              .fontWeight(index === 0 ? FontWeight.Medium : FontWeight.Regular)
              .fontColor(index === 0 ? ACCENT : INK).backgroundColor(Color.Transparent)
              .padding({ left: 12, right: 12 }).focusable(false)
              .enabled(this.connected && !this.pendingCommit && (!this.chinese || this.rimeReady))
              .onClick(() => { inputSession.selectCandidate(index, candidate); this.expanded = false; this.utility = 0; })
          }, (candidate: string, index: number) => index.toString() + ':' + candidate)
        }
      }.scrollable(ScrollDirection.Horizontal).scrollBar(BarState.Off).layoutWeight(1).height(34)
      Button(this.expanded ? '⌃' : '⌄', { type: ButtonType.Normal }).width(38).height(32).padding(0).fontSize(22)
        .fontColor(MUTED).backgroundColor(Color.Transparent).focusable(false)
        .accessibilityText(this.expanded ? '收起候选词' : '展开候选词')
        .onClick(() => { this.expanded = !this.expanded; this.utility = 0; })
    }.width('100%').height(34)
  }

  @Builder
  keyRow(index: number) {
    Row({ space: 5 }) {
      if (index === 1 && this.layer === 0) { Blank().layoutWeight(this.keyboardLayout === 'split' ? 0.3 : 0.5) }
      if (index === 2) {
        Button(this.layer === 2 ? (this.symbolPage === 0 ? '更多' : '常用') : this.capsLock ? '⇪' : '⇧', { type: ButtonType.Normal })
          .height(this.keyHeight).layoutWeight(this.keyboardLayout === 'split' ? 1.2 : 1.45).fontSize(this.layer > 0 ? 15 : 25)
          .fontColor(this.uppercase && this.layer === 0 ? ACCENT : INK)
          .backgroundColor(FUNCTION_KEY).borderRadius(7).padding(0).focusable(false).enabled(this.connected)
          .accessibilityText(this.layer > 0 ? '切换符号页' : '切换大写')
          .onClick(() => { this.shift(); })
          .gesture(LongPressGesture().onAction(() => {
            if (this.layer === 0) {
              if (this.chinese) inputSession.toggleLanguage();
              this.uppercase = true; this.capsLock = true;
            }
          }))
      }
      ForEach(this.row(index), (key: string, position: number) => {
        if (this.keyboardLayout === 'split' && position === (index === 2 ? 4 : 5)) { Blank().layoutWeight(2) }
        Button({ type: ButtonType.Normal }) {
          Column({ space: 0 }) {
            if (this.layer === 0) { Text(this.hint(index, position)).fontSize(10).fontColor(MUTED).height(13) }
            Text(this.layer === 0 && this.chinese ? key.toUpperCase() : key).fontSize(this.layer === 0 ? 22 : 20).fontColor(INK)
          }.width('100%').height('100%').justifyContent(FlexAlign.Center)
        }.height(this.keyHeight).layoutWeight(1).borderRadius(7).padding(0).backgroundColor(KEY)
          .enabled(this.connected && !this.pendingCommit && (!this.chinese || this.rimeReady)).focusable(false).accessibilityText(key)
          .onClick(() => { this.insert(key); })
          .gesture(LongPressGesture().onAction(() => {
            const hint = this.hint(index, position);
            if (hint) this.insert(hint);
          }))
      }, (key: string, position: number) => position.toString() + ':' + key)
      if (index === 1 && this.layer === 0) { Blank().layoutWeight(this.keyboardLayout === 'split' ? 0.3 : 0.5) }
      if (index === 2) {
        Button('⌫', { type: ButtonType.Normal }).height(this.keyHeight).layoutWeight(this.keyboardLayout === 'split' ? 1.2 : 1.45).fontSize(25)
          .fontColor(INK).backgroundColor(FUNCTION_KEY).borderRadius(7).padding(0).focusable(false)
          .enabled(this.connected && !this.pendingCommit && (!this.chinese || this.rimeReady)).accessibilityText('删除光标前的文字')
          .onClick(() => { inputSession.backspace(); })
          .gesture(LongPressGesture({ repeat: true }).onAction(() => { inputSession.backspace(); }))
      }
    }.width('100%').height(this.keyHeight)
  }

  @Builder
  bottomKey(label: string, weight: number, action: () => void) {
    Button(label, { type: ButtonType.Normal }).height(this.keyHeight).layoutWeight(weight)
      .fontSize(15).fontColor(INK).backgroundColor(FUNCTION_KEY).borderRadius(7).padding(0).focusable(false)
      .onClick(() => { action(); })
  }

  @Builder
  punctuation(comma: boolean) {
    Button(comma ? (this.chinese ? '，' : ',') : (this.chinese ? '。' : '.'), { type: ButtonType.Normal })
      .height(this.keyHeight).layoutWeight(0.85).fontSize(22).fontColor(INK).backgroundColor(KEY)
      .borderRadius(7).padding(0).focusable(false)
      .enabled(this.connected && !this.pendingCommit && (!this.chinese || this.rimeReady))
      .onClick(() => { this.insert(comma ? (this.chinese ? '，' : ',') : (this.chinese ? '。' : '.')); })
  }

  @Builder
  spaceKey(weight: number) {
    Button({ type: ButtonType.Normal }) {
      if (this.keyboardLayout === 'split') {
        Column({ space: 0 }) {
          Text('语音').fontSize(10).fontColor(MUTED)
          Text('⎵').fontSize(28).fontColor(INK)
        }.width('100%').height('100%').justifyContent(FlexAlign.Center)
      } else { Text(this.chinese ? '空格' : 'space').fontSize(15).fontColor(MUTED) }
    }.height(this.keyHeight).layoutWeight(weight).backgroundColor(KEY).borderRadius(7).padding(0).focusable(false)
      .accessibilityText('空格，长按语音输入')
      .enabled(this.connected && !this.pendingCommit && (!this.chinese || this.rimeReady))
      .onClick(() => { inputSession.space(); })
      .gesture(LongPressGesture().onAction(() => { if (!this.password && this.voicePhase === 'idle') this.voice(); }))
  }

  @Builder
  bottomRow() {
    Row({ space: 5 }) {
      this.bottomKey(this.layer > 0 ? '返回' : '符号', this.keyboardLayout === 'split' ? 1.2 : 1.1,
        () => { this.layer = this.layer > 0 ? 0 : 2; this.symbolPage = 0; })
      Button(this.chinese ? '中/英' : '英/中', { type: ButtonType.Normal }).height(this.keyHeight).layoutWeight(0.95)
        .fontSize(14).fontColor(INK).backgroundColor(FUNCTION_KEY).borderRadius(7).padding(0).focusable(false)
        .enabled(this.connected && !this.password).onClick(() => { this.language(); })
        .gesture(LongPressGesture().onAction(() => { this.inputMethods(); }))
      if (this.keyboardLayout === 'split') { this.bottomKey('◎', 0.8, () => { this.inputMethods(); }) }
      else { this.bottomKey('123', 0.9, () => { this.layer = this.layer === 1 ? 0 : 1; }) }
      this.punctuation(true)
      if (this.keyboardLayout === 'split') {
        this.spaceKey(1.1)
        Blank().layoutWeight(2)
        this.spaceKey(1.1)
      } else { this.spaceKey(3) }
      this.punctuation(false)
      if (this.keyboardLayout === 'split') {
        this.bottomKey('☺', 0.8, () => { this.utility = this.utility === 2 ? 0 : 2; })
        this.bottomKey('123', 0.9, () => { this.layer = this.layer === 1 ? 0 : 1; })
      }
      Button(this.enterLabel, { type: ButtonType.Normal }).height(this.keyHeight).layoutWeight(1.25)
        .fontSize(16).fontColor(this.keyboardLayout === 'split' ? INK : '#FFFFFF')
        .backgroundColor(this.keyboardLayout === 'split' ? FUNCTION_KEY : ACCENT).borderRadius(7).padding(0).focusable(false)
        .enabled(this.connected && !this.pendingCommit && (!this.chinese || this.rimeReady)).onClick(() => { inputSession.enter(); })
    }.width('100%').height(this.keyHeight)
  }

  @Builder
  digitRow() {
    Row({ space: 5 }) {
      ForEach(['1', '2', '3', '4', '5', '6', '7', '8', '9', '0'], (digit: string, index: number) => {
        if (this.keyboardLayout === 'split' && index === 5) { Blank().layoutWeight(2) }
        Button(digit, { type: ButtonType.Normal }).height(this.keyHeight).layoutWeight(1).fontSize(24)
          .fontColor(INK).backgroundColor(KEY).borderRadius(7).padding(0).focusable(false)
          .enabled(this.connected && !this.pendingCommit && (!this.chinese || this.rimeReady))
          .onClick(() => { this.insert(digit); })
      }, (digit: string) => digit)
    }.height(this.keyHeight).width('100%')
  }

  @Builder
  layoutChooser() {
    Column({ space: 14 }) {
      Text('键盘布局').fontSize(17).fontColor(INK)
      Row({ space: 10 }) {
        ForEach(['standard', 'split', 'floating'], (mode: string, index: number) => {
          Button(['标准键盘', '分离键盘', '悬浮键盘'][index], { type: ButtonType.Normal })
            .height(48).layoutWeight(1).fontSize(15).borderRadius(8).padding(0).focusable(false)
            .fontColor(this.keyboardLayout === mode ? '#FFFFFF' : INK)
            .backgroundColor(this.keyboardLayout === mode ? ACCENT : KEY)
            .onClick(() => { this.layer = 0; this.expanded = false; this.utility = 0; panelLayout.select(mode); })
        }, (mode: string) => mode)
      }.width('100%')
      Text('悬浮键盘可拖动顶部横条，使用 − / + 调整大小').fontSize(13).fontColor(MUTED)
      Button('返回键盘').focusable(false).fontColor(ACCENT).backgroundColor(KEY)
        .onClick(() => { this.utility = 0; })
    }.width('100%').layoutWeight(1).justifyContent(FlexAlign.Center).padding(8)
  }

  @Builder
  floatingHandle() {
    Row({ space: 4 }) {
      Button('−').height(24).layoutWeight(1).fontSize(18).padding(0).focusable(false)
        .fontColor(INK).backgroundColor(Color.Transparent).accessibilityText('缩小悬浮键盘')
        .onClick(() => { panelLayout.resize(-0.1); })
      Row() { Text('━━━━').fontSize(16).fontColor(MUTED) }
        .height(24).layoutWeight(5).justifyContent(FlexAlign.Center).accessibilityText('拖动悬浮键盘')
        .gesture(PanGesture().onActionStart(() => { panelLayout.beginMove(); })
          .onActionUpdate((event: GestureEvent) => { panelLayout.drag(event.offsetX, event.offsetY); })
          .onActionEnd(() => { panelLayout.endMove(); })
          .onActionCancel(() => { panelLayout.endMove(); }))
      Button('+').height(24).layoutWeight(1).fontSize(18).padding(0).focusable(false)
        .fontColor(INK).backgroundColor(Color.Transparent).accessibilityText('放大悬浮键盘')
        .onClick(() => { panelLayout.resize(0.1); })
      Button('↙').height(24).layoutWeight(1).fontSize(18).padding(0).focusable(false)
        .fontColor(INK).backgroundColor(Color.Transparent).accessibilityText('恢复固定键盘')
        .onClick(() => { panelLayout.select('standard'); })
    }.height(24).width('100%')
  }

  @Builder
  numberPad() {
    Row({ space: 6 }) {
      Column({ space: 6 }) {
        ForEach(['+', '-', '×', '÷'], (value: string) => {
          Button(value, { type: ButtonType.Normal }).height(52).width('100%').fontSize(22)
            .fontColor(INK).backgroundColor(FUNCTION_KEY).borderRadius(7).padding(0).focusable(false)
            .enabled(this.connected && !this.pendingCommit && (!this.chinese || this.rimeReady)).onClick(() => { this.insert(value); })
        }, (value: string) => value)
      }.layoutWeight(1)
      Column({ space: 6 }) {
        ForEach([['1', '2', '3'], ['4', '5', '6'], ['7', '8', '9']], (row: string[], index: number) => {
          Row({ space: 6 }) {
            ForEach(row, (value: string) => {
              Button(value, { type: ButtonType.Normal }).height(52).layoutWeight(1).fontSize(25)
                .fontColor(INK).backgroundColor(KEY).borderRadius(7).padding(0).focusable(false)
                .enabled(this.connected && !this.pendingCommit && (!this.chinese || this.rimeReady)).onClick(() => { this.insert(value); })
            }, (value: string) => value)
          }.width('100%').height(52)
        }, (row: string[], index: number) => index.toString())
        Row({ space: 6 }) {
          Button('返回', { type: ButtonType.Normal }).height(52).layoutWeight(1).fontSize(14)
            .fontColor(INK).backgroundColor(FUNCTION_KEY).borderRadius(7).padding(0).focusable(false)
            .onClick(() => { this.layer = 0; })
          Button('0', { type: ButtonType.Normal }).height(52).layoutWeight(1).fontSize(25)
            .fontColor(INK).backgroundColor(KEY).borderRadius(7).padding(0).focusable(false)
            .enabled(this.connected && !this.pendingCommit && (!this.chinese || this.rimeReady)).onClick(() => { this.insert('0'); })
          Button('00', { type: ButtonType.Normal }).height(52).layoutWeight(1).fontSize(22)
            .fontColor(INK).backgroundColor(KEY).borderRadius(7).padding(0).focusable(false)
            .enabled(this.connected && !this.pendingCommit && (!this.chinese || this.rimeReady)).onClick(() => { this.insert('00'); })
        }.height(52).width('100%')
      }.layoutWeight(3.6)
      Column({ space: 6 }) {
        Button('⌫', { type: ButtonType.Normal }).height(52).width('100%').fontSize(25)
          .fontColor(INK).backgroundColor(FUNCTION_KEY).borderRadius(7).padding(0).focusable(false)
          .enabled(this.connected && !this.pendingCommit && (!this.chinese || this.rimeReady)).accessibilityText('删除光标前的文字')
          .onClick(() => { inputSession.backspace(); })
          .gesture(LongPressGesture({ repeat: true }).onAction(() => { inputSession.backspace(); }))
        Button('.', { type: ButtonType.Normal }).height(52).width('100%').fontSize(25)
          .fontColor(INK).backgroundColor(KEY).borderRadius(7).padding(0).focusable(false)
          .enabled(this.connected && !this.pendingCommit && (!this.chinese || this.rimeReady)).onClick(() => { this.insert('.'); })
        Button(this.enterLabel, { type: ButtonType.Normal }).height(110).width('100%').fontSize(17)
          .fontColor('#FFFFFF').backgroundColor(ACCENT).borderRadius(7).padding(0).focusable(false)
          .enabled(this.connected && !this.pendingCommit && (!this.chinese || this.rimeReady)).onClick(() => { inputSession.enter(); })
      }.layoutWeight(1.2)
    }.width('100%').layoutWeight(1).alignItems(VerticalAlign.Center)
  }

  @Builder
  expandedCandidates() {
    Column({ space: 8 }) {
      Scroll() {
        Flex({ wrap: FlexWrap.Wrap }) {
          ForEach(this.candidates, (candidate: string, index: number) => {
            Button(candidate, { type: ButtonType.Normal }).height(48).fontSize(22)
              .fontColor(index === 0 ? ACCENT : INK).backgroundColor(KEY).borderRadius(7)
              .padding({ left: 18, right: 18 }).margin({ right: 6, bottom: 6 }).focusable(false)
              .enabled(this.connected && !this.pendingCommit && (!this.chinese || this.rimeReady))
              .onClick(() => { inputSession.selectCandidate(index, candidate); this.expanded = false; this.utility = 0; })
          }, (candidate: string, index: number) => index.toString() + ':' + candidate)
        }.width('100%')
      }.width('100%').layoutWeight(1).scrollBar(BarState.Off)
      Row({ space: 12 }) {
        Button('上一页').enabled(this.page > 0 && !this.pendingCommit).focusable(false)
          .fontColor(INK).backgroundColor(KEY).onClick(() => { inputSession.page(false); })
        Text((this.page + 1).toString()).fontSize(14).fontColor(MUTED)
        Button('下一页').enabled(this.hasNext && !this.pendingCommit).focusable(false)
          .fontColor(INK).backgroundColor(KEY).onClick(() => { inputSession.page(true); })
        Blank()
        Button('返回键盘').focusable(false).fontColor(ACCENT).backgroundColor(KEY)
          .onClick(() => { this.expanded = false; })
      }.height(42).width('100%')
    }.width('100%').layoutWeight(1).padding({ top: 8 })
  }

  @Builder
  tools() {
    Column({ space: 16 }) {
      Text('键盘工具').fontSize(17).fontWeight(FontWeight.Medium).fontColor(INK)
      Button('键盘布局').focusable(false).fontColor(INK).backgroundColor(KEY)
        .onClick(() => { this.utility = 3; })
      Button(this.cloudEnabled ? '云候选：开（Google）' : '云候选：关').focusable(false).fontColor(INK).backgroundColor(KEY)
        .onClick(() => { inputSession.toggleCloudCandidates(); })
      Text('开启后将当前拼音发送给 Google，密码框不查询').fontSize(12).fontColor(MUTED)
      Row({ space: 12 }) {
        Button('OpenLess 设置').focusable(false).fontColor(INK).backgroundColor(KEY)
          .onClick(() => { inputSession.settings(); })
        Button('切换输入法').focusable(false).fontColor(INK).backgroundColor(KEY)
          .onClick(() => { this.inputMethods(); })
      }
      Button('返回键盘').focusable(false).fontColor(ACCENT).backgroundColor(KEY)
        .onClick(() => { this.utility = 0; })
    }.width('100%').layoutWeight(1).justifyContent(FlexAlign.Center)
  }

  @Builder
  emoji() {
    Column({ space: 8 }) {
      Scroll() {
        Flex({ wrap: FlexWrap.Wrap, justifyContent: FlexAlign.Center }) {
          ForEach(['😀', '😄', '😊', '🥰', '😍', '😎', '🤔', '😂', '🥹', '😭', '😅', '😴', '👍', '👏', '🙏', '💪', '❤️', '🎉', '🌹', '✨', '✅', '💡', '☀️', '🎂'], (value: string) => {
            Button(value, { type: ButtonType.Normal }).width(52).height(50).fontSize(27)
              .backgroundColor(Color.Transparent).focusable(false).enabled(this.connected && !this.pendingCommit && (!this.chinese || this.rimeReady))
              .onClick(() => { inputSession.insert(value); })
          }, (value: string) => value)
        }.width('100%')
      }.width('100%').layoutWeight(1).scrollBar(BarState.Off)
      Button('返回键盘').focusable(false).fontColor(ACCENT).backgroundColor(KEY)
        .onClick(() => { this.utility = 0; })
    }.width('100%').layoutWeight(1)
  }

  @Builder
  voicePanel() {
    Column({ space: 16 }) {
      if (this.voicePhase === 'review' || this.voicePhase === 'inserting') {
        Scroll() { Text(this.voiceText).fontSize(22).fontColor(INK).width('100%') }
          .width('100%').layoutWeight(1).padding(12)
        Row({ space: 12 }) {
          Button('重新输入').focusable(false).enabled(this.voicePhase === 'review')
            .fontColor(INK).backgroundColor(KEY).onClick(() => { inputSession.cancelVoice(); })
          Button(this.voicePhase === 'inserting' ? '正在写入' : '确认写入').focusable(false)
            .enabled(this.voicePhase === 'review').fontColor('#FFFFFF').backgroundColor(ACCENT)
            .onClick(() => { inputSession.confirmVoice(); })
        }
      } else {
        Text(this.voicePhase === 'recording' ? '正在听你说' : this.voicePhase === 'processing' ? '正在识别…' : '正在准备…')
          .fontSize(22).fontColor(INK)
        if (this.voicePhase === 'recording') {
          Progress({ value: this.voiceLevel * 100, total: 100, type: ProgressType.Linear })
            .width('64%').color(ACCENT).backgroundColor(FUNCTION_KEY)
          Button('说完了').focusable(false).fontColor('#FFFFFF').backgroundColor(ACCENT)
            .onClick(() => { inputSession.stopVoice(); })
        }
        Button('返回键盘').focusable(false).fontColor(MUTED).backgroundColor(Color.Transparent)
          .onClick(() => { inputSession.cancelVoice(); })
      }
    }.width('100%').layoutWeight(1).justifyContent(FlexAlign.Center).padding(12)
  }

  build() {
    Column({ space: 6 }) {
      if (this.keyboardLayout === 'floating') { this.floatingHandle() }
      Column({ space: 0 }) {
        if (this.preedit || this.pendingCommit ||
          (this.status !== '可输入' && this.status !== 'Rime 拼音' && this.status !== '等待输入框') ||
          (this.connected && this.chinese && !this.rimeReady)) {
          Row() {
            Text(this.preedit || (this.connected && this.chinese && !this.rimeReady && this.status === '可输入' ?
              '正在准备中文输入…' : this.status))
              .fontSize(12).fontColor(MUTED).maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis }).layoutWeight(1)
          }.height(14).width('100%').padding({ left: 8, right: 8 })
        }
        if (this.pendingCommit) { this.pendingBar() }
        else if (this.candidates.length > 0 && this.voicePhase === 'idle') { this.candidateBar() }
        else { this.keyboardToolbar() }
      }.height(48).width('100%').justifyContent(FlexAlign.End)
      if (this.voicePhase !== 'idle') { this.voicePanel() }
      else if (this.utility === 1) { this.tools() }
      else if (this.utility === 2) { this.emoji() }
      else if (this.utility === 3) { this.layoutChooser() }
      else if (this.expanded && this.candidates.length > 0) { this.expandedCandidates() }
      else if (this.layer === 1) { this.numberPad() }
      else {
        Column({ space: 6 }) {
          if (this.showNumberRow && this.layer === 0) { this.digitRow() }
          this.keyRow(0)
          this.keyRow(1)
          this.keyRow(2)
          this.bottomRow()
        }.width('100%').layoutWeight(1).justifyContent(FlexAlign.End)
      }
    }.width('100%').height('100%').padding({ left: 6, right: 6, top: 4, bottom: 8 }).backgroundColor(BACKGROUND)
      .borderRadius(this.keyboardLayout === 'floating' ? 12 : 0).clip(true)
  }
}
