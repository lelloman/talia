// Reproducible catalog changes; piping this file to a save is intentionally not automatic.
import {readFileSync} from 'node:fs';import {fileURLToPath} from 'node:url';
import '../engine/shared/value.js';
const load=p=>readFileSync(new URL('../dashboard/examples/'+p,import.meta.url),'utf8');
export const hosts=[{host:'homelab',job:'node-exporter',instance:'node-exporter:9100'},{host:'vps-eu',job:'node-exporter-vps',instance:'vps-eu'},{host:'vps-us',job:'node-exporter-vps',instance:'vps-us'}];
const put=(kind,id,document)=>({op:'put',key:{kind,id},document});
export const changes=[
 put('ui','host-metric-card',{source:load('host/metric-card.ui')}),
 put('ui','host-layout',{source:load('host/layout.ui'),references:[{kind:'ui',id:'host-metric-card'}]}),
 put('function','host-present',{source:load('host/present.js')}),
 put('monitor_definition','host-collect',{id:'host-collect',version:3,kind:'pipeline',source:load('host/collect.js')}),
 put('variable_definition','host-snapshot',{id:'host-snapshot',version:1,kind:'stored',source:'',value_schema:'any',state_schema:'any',dependencies:[]}),
 ...hosts.flatMap(p=>[
  put('variable','host-'+p.host,{id:'host-'+p.host,definition:'host-snapshot',params:TaliaValue.encode({}),history_count:120,history_age_ms:3600000}),
  put('monitor_instance','host-'+p.host,{id:'host-'+p.host,definition:'host-collect',sources:{prom:'homelab-prometheus'},outputs:{summary:'host-'+p.host},params:TaliaValue.encode(p),schedule:{kind:'interval',every_ms:30000},stale_after_ms:90000}),
  put('dashboard',p.host,{ui:load('host/dashboard.ui'),view_model:load('host/dashboard.vm.js'),references:[{kind:'ui',id:'host-layout'},{kind:'function',id:'host-present'}],params:{host:p.host,summary:'host-'+p.host},grants:{reads:['host-'+p.host],writes:[],runs:[]}})
 ])
];
if(process.argv[1]===fileURLToPath(import.meta.url))console.log(JSON.stringify(changes,null,2));
