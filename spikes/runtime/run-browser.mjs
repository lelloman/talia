import {chromium} from 'playwright';
import http from 'node:http';
import {readFile} from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
const root=path.dirname(fileURLToPath(import.meta.url));
const server=http.createServer(async(req,res)=>{
 try{
  const file=path.resolve(root,'.'+new URL(req.url,'http://localhost').pathname);
  if(!file.startsWith(root+path.sep))throw Error('invalid path');
  const bytes=await readFile(file);
  res.setHeader('Content-Type',file.endsWith('.js')?'application/javascript':file.endsWith('.wasm')?'application/wasm':'text/html');res.end(bytes);
 }catch{res.writeHead(404);res.end();}
});
await new Promise(r=>server.listen(0,'127.0.0.1',r));
let browser;
try{
 browser=await chromium.launch({headless:true,...(process.env.TALIA_CHROMIUM?{executablePath:process.env.TALIA_CHROMIUM}:{})});
 const page=await browser.newPage();await page.goto(`http://127.0.0.1:${server.address().port}/web/index.html`);
 await page.waitForFunction(()=>window.spikeResult,null,{timeout:20000});
 const result=await page.evaluate(()=>window.spikeResult);result.browser=browser.version();
 if(result.error||result.renderer_ticks<1)throw Error(JSON.stringify(result));
 console.log(JSON.stringify(result));
 if(result.qualified === false) process.exitCode = 2;
}finally{await browser?.close();server.close();}
