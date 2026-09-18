import {newQuickJSWASMModule as oldModule} from 'quickjs-emscripten';
import {newQuickJSWASMModule as newModule} from 'quickjs-new';
import ng from '@jitl/quickjs-ng-wasmfile-release-sync';
import {probeMemory} from './memory-probe.mjs';
const result = {};
for (const [name, create] of [
  ['bellard_0_31_0', () => oldModule()],
  ['bellard_0_32_0', () => newModule()],
  ['quickjs_ng_0_32_0', () => newModule(ng)],
]) {
  result[name] = probeMemory(await create());
}
console.log(JSON.stringify(result,null,2));
