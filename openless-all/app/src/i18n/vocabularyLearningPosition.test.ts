import assert from 'node:assert/strict';
import { de } from './de';
import { en } from './en';
import { es } from './es';
import { fr } from './fr';
import { ja } from './ja';
import { ko } from './ko';
import { zhCN } from './zh-CN';
import { zhTW } from './zh-TW';

for (const [locale, messages] of Object.entries({ de, en, es, fr, ja, ko, zhCN, zhTW })) {
  const learning = messages.settings.vocabularyLearning;
  for (const key of [
    'suggestionPosition',
    'suggestionPositionHint',
    'positionNearInput',
    'positionBottomRight',
  ] as const) {
    assert.ok(learning[key].trim(), `${locale}: missing vocabulary learning ${key}`);
  }
  assert.notEqual(learning.positionNearInput, learning.positionBottomRight);
  assert.match(learning.suggestionPositionHint, /Windows/);
}

console.log('vocabulary learning position translations cover both Windows choices');
