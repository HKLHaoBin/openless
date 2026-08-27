import { mockSettings } from './mock-data';

// 系统代理开关默认开启，与后端 serde 默认值保持一致（issue #869）。
if (mockSettings.useSystemProxy !== true) {
  throw new Error(`mockSettings.useSystemProxy must default to true, got ${mockSettings.useSystemProxy}`);
}
if (mockSettings.httpConnectTimeoutSecs !== 8) {
  throw new Error(`mockSettings.httpConnectTimeoutSecs must default to 8, got ${mockSettings.httpConnectTimeoutSecs}`);
}
if (mockSettings.httpPoolIdleTimeoutSecs !== 300) {
  throw new Error(`mockSettings.httpPoolIdleTimeoutSecs must default to 300, got ${mockSettings.httpPoolIdleTimeoutSecs}`);
}
if (mockSettings.httpRequestTimeoutSecs !== 30) {
  throw new Error(`mockSettings.httpRequestTimeoutSecs must default to 30, got ${mockSettings.httpRequestTimeoutSecs}`);
}

console.log('mock-data.test.ts passed');
