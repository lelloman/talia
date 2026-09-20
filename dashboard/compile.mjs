import {readFileSync,writeFileSync,mkdirSync,renameSync} from 'node:fs';import {dirname,resolve} from 'node:path';import {createHash} from 'node:crypto';import vm from 'node:vm';import './shared/ui.js';
const manifestPath=process.argv[2]||'dashboard/examples/monitor.package.json',output=process.argv[3]||'dashboard/generated/monitor.json';
const manifest=JSON.parse(readFileSync(manifestPath,'utf8')),root=dirname(resolve(manifestPath));
const load=path=>readFileSync(resolve(root,path),'utf8');
const definitions=Object.fromEntries(Object.entries(manifest.definitions||{}).map(([name,path])=>[name,TaliaUI.compileDefinition(load(path))]));
const viewModel=(manifest.scripts||[]).map(load).concat(load(manifest.viewModel)).join('\n');new vm.Script(viewModel,{filename:manifest.viewModel});
const pkg={version:1,id:manifest.id,ui:TaliaUI.compile(load(manifest.ui)),definitions,viewModel,params:manifest.params||{}};
pkg.revision=createHash('sha256').update(JSON.stringify(pkg)).digest('hex');TaliaUI.validatePackage(pkg);
mkdirSync(dirname(output),{recursive:true});const temporary=output+'.'+process.pid+'.tmp';writeFileSync(temporary,JSON.stringify(pkg,null,2)+'\n');renameSync(temporary,output);console.log(pkg.revision);
