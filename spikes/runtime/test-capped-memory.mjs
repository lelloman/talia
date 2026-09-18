import {probeCappedMemory} from './capped-memory.mjs';
console.log(JSON.stringify(await probeCappedMemory(), null, 2));
