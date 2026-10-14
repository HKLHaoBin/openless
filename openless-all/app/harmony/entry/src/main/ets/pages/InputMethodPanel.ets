import { inputMethod } from '@kit.IMEKit';
import { imeStorage, inputSession } from '../inputmethod/InputSession';

@Entry(imeStorage)
@Component
struct InputMethodPanel {
  @LocalStorageLink('connected') connected: boolean = false;
  @LocalStorageLink('status') status: string = '等待输入框';
  @LocalStorageLink('enterLabel') enterLabel: string = '换行';
  @LocalStorageLink('chinese') chinese: boolean = true;
  @LocalStorageLink('preedit') preedit: string = '';
  @LocalStorageLink('candidates') candidates: string[] = [];
  @LocalStorageLink('rimeReady') rimeReady: boolean = false;
  @LocalStorageLink('page') page: number = 0;
  @LocalStorageLink('hasNext') hasNext: boolean = false;
  @LocalStorageLink('password') password: boolean = false;
  @LocalStorageLink('voicePhase') voicePhase: string = 'idle';
  @LocalStorageLink('voiceText') voiceText: string = '';
  @LocalStorageLink('voiceLevel') voiceLevel: number = 0;
  @LocalStorageLink('pendingCommit') pendingCommit: boolean = false;
  @State uppercase: boolean = false;
  @State symbols: boolean = false;

  private row(index: number): string[] {
    const letters: string[] = ['qwertyuiop', 'asdfghjkl', 'zxcvbnm'];
    const numbers: string[][] = [
      ['1', '2', '3', '4', '5', '6', '7', '8', '9', '0'],
      ['@', '#', '￥', '%', '&', '*', '(', ')', '“', '”'],
      ['，', '。', '？', '！', '：', '；', '、']
    ];
    return this.symbols ? numbers[index] : (this.uppercase ? letters[index].toUpperCase() : letters[index]).split('');
  }

  @Builder
  keyRow(index: number) {
    Row({ space: 5 }) {
      if (index === 2) {
        Button(this.symbols ? 'ABC' : (this.uppercase ? '⇧' : '↑'))
          .height(44).layoutWeight(1.35).fontColor('#333333').backgroundColor('#DFE2E7')
          .onClick(() => { if (this.symbols) this.symbols = false; else this.uppercase = !this.uppercase; })
      }
      ForEach(this.row(index), (key: string) => {
        Button(key, { type: ButtonType.Normal })
          .height(44).layoutWeight(1).borderRadius(7).fontSize(19)
          .fontColor('#202124').backgroundColor('#FFFFFF').enabled(this.connected)
          .focusable(false)
          .onClick(() => { inputSession.insert(key); })
      }, (key: string) => key)
      if (index === 2) {
        Button('删除', { type: ButtonType.Normal }).height(44).layoutWeight(1.35).fontSize(14).borderRadius(7)
          .focusable(false).accessibilityText('删除光标前的文字')
          .fontColor('#333333').backgroundColor('#DFE2E7').enabled(this.connected)
          .onClick(() => { inputSession.backspace(); })
          .gesture(LongPressGesture({ repeat: true }).onAction(() => { inputSession.backspace(); }))
      }
    }.width('100%').justifyContent(FlexAlign.Center)
  }

  build() {
    Column({ space: 8 }) {
      Row() {
        Text('OpenLess').fontSize(17).fontWeight(FontWeight.Bold).fontColor('#202124')
        Blank()
        Button('设置').fontSize(13).fontColor('#555555').backgroundColor(Color.Transparent)
          .onClick(() => { inputSession.settings(); })
        Button('收起').fontSize(13).fontColor('#555555').backgroundColor(Color.Transparent)
          .onClick(() => { inputSession.hide(); })
      }.width('100%').height(34)
      Row() {
        Text(this.preedit || this.status).fontSize(12).fontColor('#6B7280').maxLines(1).textOverflow({ overflow: TextOverflow.Ellipsis })
        Blank()
        Button(this.voicePhase === 'recording' ? '结束录音' : '语音').fontSize(12).height(28).focusable(false)
          .enabled(this.connected && !this.password && (this.voicePhase === 'idle' || this.voicePhase === 'recording'))
          .fontColor('#FFFFFF').backgroundColor('#202124')
          .onClick(() => { if (this.voicePhase === 'recording') inputSession.stopVoice(); else inputSession.startVoice(); })
      }.width('100%').height(30)
      if (this.voicePhase !== 'idle') {
        Column({ space: 12 }) {
          if (this.voicePhase === 'review' || this.voicePhase === 'inserting') {
            Scroll() { Text(this.voiceText).fontSize(20).fontColor('#202124').width('100%') }
              .height(140).width('100%')
            Row({ space: 12 }) {
              Button('确认写入').focusable(false).enabled(this.voicePhase === 'review')
                .onClick(() => { inputSession.confirmVoice(); })
              Button('丢弃').focusable(false).enabled(this.voicePhase === 'review')
                .onClick(() => { inputSession.cancelVoice(); })
            }
          } else {
            Text(this.voicePhase === 'recording' ? '正在听你说话' : this.voicePhase === 'processing' ? '正在识别，请稍候' : '正在准备录音')
              .fontSize(20).fontColor('#202124')
            if (this.voicePhase === 'recording') {
              Progress({ value: this.voiceLevel * 100, total: 100, type: ProgressType.Linear })
                .width('80%').color('#202124')
            }
            Button('取消').focusable(false).onClick(() => { inputSession.cancelVoice(); })
          }
        }.height(210).width('100%').justifyContent(FlexAlign.Center)
      } else {
        if (this.pendingCommit) {
          Button('文字未写入 · 重新确认').focusable(false).height(32)
            .onClick(() => { inputSession.retryCommit(); })
        }
      Row({ space: 4 }) {
        Scroll() {
          Row({ space: 4 }) {
            ForEach(this.candidates, (candidate: string, index: number) => {
              Button(candidate, { type: ButtonType.Normal }).fontSize(18).height(34)
                .fontColor('#202124').backgroundColor('#FFFFFF').borderRadius(6).focusable(false)
                .onClick(() => { inputSession.selectCandidate(index); })
            }, (candidate: string, index: number) => index.toString() + ':' + candidate)
          }
        }.scrollable(ScrollDirection.Horizontal).layoutWeight(1).height(36)
        Button('‹').height(34).enabled(this.page > 0).onClick(() => { inputSession.page(false); })
        Button('›').height(34).enabled(this.hasNext).onClick(() => { inputSession.page(true); })
      }.width('100%').height(36)
      this.keyRow(0)
      this.keyRow(1)
      this.keyRow(2)
      Row({ space: 5 }) {
        Button(this.symbols ? 'ABC' : '123').height(44).layoutWeight(1.2)
          .fontColor('#333333').backgroundColor('#DFE2E7').onClick(() => { this.symbols = !this.symbols; })
        Button(this.chinese ? '中' : 'EN').height(44).layoutWeight(1).fontColor('#333333').backgroundColor('#DFE2E7')
          .enabled(this.connected && !this.password).onClick(() => { inputSession.toggleLanguage(); })
        Button('切换').height(44).layoutWeight(1.2).fontColor('#333333').backgroundColor('#DFE2E7')
          .onClick(() => { inputMethod.getSetting().showOptionalInputMethods().catch(() => { this.status = '请在系统输入法菜单切换'; }); })
        Button('空格').height(44).layoutWeight(4).fontColor('#333333').backgroundColor('#FFFFFF')
          .enabled(this.connected).onClick(() => { inputSession.space(); })
        Button(this.enterLabel).height(44).layoutWeight(1.5).fontSize(14).fontColor('#FFFFFF').backgroundColor('#202124')
          .enabled(this.connected).onClick(() => { inputSession.enter(); })
      }.width('100%')
      }
    }.width('100%').height('100%').padding({ left: 8, right: 8, top: 6, bottom: 12 }).backgroundColor('#F0F1F4')
  }
}
