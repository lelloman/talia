// Reproducible catalog changes; piping this file to a save is intentionally not automatic.
import {readFileSync} from 'node:fs';import {fileURLToPath} from 'node:url';
import '../engine/shared/value.js';
const load=p=>readFileSync(new URL('../dashboard/examples/'+p,import.meta.url),'utf8');
export const hosts=[{host:'homelab',networkDevice:'enp3s0',networkHostProbe:true,gitWorkspaces:true,job:'node-exporter',instance:'node-exporter:9100'},{host:'vps-eu',networkDevice:'ens6',job:'node-exporter-vps',instance:'vps-eu'},{host:'vps-us',networkDevice:'ens6',job:'node-exporter-vps',instance:'vps-us'}];
const put=(kind,id,document)=>({op:'put',key:{kind,id},document});
export const changes=[
 put('ui','host-metric-card',{source:load('host/metric-card.ui')}),
 put('ui','host-layout',{source:load('host/layout.ui'),references:[{kind:'ui',id:'host-metric-card'}]}),
 put('function','host-present',{source:load('host/present.js')}),
 put('monitor_definition','host-collect',{id:'host-collect',version:10,kind:'pipeline',source:load('host/collect.js')}),
 put('variable_definition','host-snapshot',{id:'host-snapshot',version:1,kind:'stored',source:'',value_schema:'any',state_schema:'any',dependencies:[]}),
 put('data_source','homelab-git-trigger',{id:'homelab-git-trigger',kind:'http',url:'http://talia-git-probe-trigger:8080',timeout_ms:5000,max_bytes:4096}),
 put('monitor_definition','git-recheck',{id:'git-recheck',version:1,kind:'pipeline',source:readFileSync(new URL('./git-workspaces/recheck.js',import.meta.url),'utf8')}),
 put('monitor_instance','git-recheck-homelab',{id:'git-recheck-homelab',definition:'git-recheck',sources:{trigger:'homelab-git-trigger'},params:TaliaValue.encode({}),timeout_ms:180000}),
 ...hosts.flatMap(p=>[
  put('variable','host-'+p.host,{id:'host-'+p.host,definition:'host-snapshot',params:TaliaValue.encode({}),history_count:120,history_age_ms:3600000}),
  put('monitor_instance','host-'+p.host,{id:'host-'+p.host,definition:'host-collect',sources:{prom:'homelab-prometheus'},outputs:{summary:'host-'+p.host},params:TaliaValue.encode(p),schedule:{kind:'interval',every_ms:30000},stale_after_ms:90000}),
  put('dashboard',p.host,{ui:load('host/dashboard.ui'),view_model:load('host/dashboard.vm.js'),references:[{kind:'ui',id:'host-layout'},{kind:'function',id:'host-present'}],params:{host:p.host,summary:'host-'+p.host,...(p.gitWorkspaces?{gitRecheck:'git-recheck-'+p.host}:{})},grants:{reads:['host-'+p.host,...(p.gitWorkspaces?['monitor.git-recheck-'+p.host]:[])],writes:[],runs:p.gitWorkspaces?['git-recheck-'+p.host]:[]}})
 ])
];
if(process.argv[1]===fileURLToPath(import.meta.url))console.log(JSON.stringify(changes,null,2));
