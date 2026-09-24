import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import '../shared/ui.js';
const {compile,resolve}=globalThis.TaliaUI;
const wrap=body=>`<Dashboard id="d"><Surface id="s"><Column id="c">${body}</Column></Surface></Dashboard>`;
test('complete example compiles and binds without evaluating source',()=>{
 const tree=compile(readFileSync(new URL('../examples/monitor.ui',import.meta.url),'utf8'));
 const result=resolve(tree,{screen:'overviewScreen',status:'Ready',title:{text:'Talìa'},history:[1,2],services:[]},{params:{sidebar:false},definitions:{notice:TaliaUI.compileDefinition('<Text id="noticeText" text={params.text}/>')}});
 assert.equal(tree.version,1);assert.equal(result.children[0].children.length,2);
});
test('compiler diagnoses unsafe expressions and invalid structures',()=>{
 for(const body of ['<Text id="x" text={globalThis.attack()} />','<Text id="x" text={state.constructor} />','<Text id="x" nope="x"/>','<Text id="x" text="x"/><Text id="x" text="y"/>','<If id="i" when={true}/>','<Text id="x" text={item.label}/>','<Slider id="s" min={10} max={1} value={3} label="s" onChange={actions.a}/>'])assert.throws(()=>compile(wrap(body)),/^Error: \d+:\d+:/);
 assert.throws(()=>compile(wrap('<Text id="x" text="ok" />')+'execute()'),/trailing/);
});
test('dynamic types, missing paths, repeat keys and responsive units are checked',()=>{
 const tree=compile(wrap('<For id="list" items={state.items} key={item.id}><Text id="row" text={item.label}/></For>'));
 const a={id:'a',label:'A'},b={id:'b',label:'B'};
 const nodes=items=>resolve(tree,{items}).children[0].children[0].children;
 assert.equal(nodes([a,b])[0].id,nodes([b,a])[1].id);
 assert.throws(()=>nodes([a,a]),/duplicate repeated key/);assert.throws(()=>nodes([{label:'x'}]),/missing binding/);
 assert.throws(()=>resolve(tree,{items:3}),/expected array/);
 const responsive=compile(wrap('<Width id="w" min="600dp"><Text id="t" text="wide"/></Width>'));
 assert.equal(resolve(responsive,{}, {width:900,scale:2}).children[0].children[0].props.visibility,'collapsed');
 assert.equal(resolve(responsive,{}, {width:1200,scale:2}).children[0].children[0].children.length,1);
});
test('unknown references and malformed precompiled versions are rejected',()=>{
 const tree=compile(wrap('<ScreenRef id="r" screen="absent"/>'));
 assert.throws(()=>resolve(tree,{}),/unknown screen/);
 assert.throws(()=>resolve({version:2},{}),/version/);
});
test('reusable definitions are validated, scoped and parameterized',()=>{
 const t=compile(wrap('<Use id="a" definition="notice" params={state.a}/><Use id="b" definition="notice" params={state.b}/>'));
 const definitions={notice:TaliaUI.compileDefinition('<Text id="text" text={params.text}/>')};
 const rendered=resolve(t,{a:{text:'A'},b:{text:'B'}},{definitions});
 assert.equal(rendered.children[0].children[0].children[0].props.text,'A');assert.equal(rendered.children[0].children[1].children[0].props.text,'B');
 assert.notEqual(rendered.children[0].children[0].children[0].id,rendered.children[0].children[1].children[0].id);
 const invalid={notice:{...definitions.notice,type:'Script'}};assert.throws(()=>resolve(t,{},{definitions:invalid}),/unknown component/);
 const cyclic={notice:TaliaUI.compileDefinition('<Use id="again" definition="notice" params={params.x}/>')};assert.throws(()=>resolve(t,{},{definitions:cyclic}),/reference cycle/);
});
test('semantic presentation is bounded and repeated grids preserve identity',()=>{
 const tree=compile(wrap('<For id="cards" items={state.items} key={item.id} columns={2} gap="12dp"><Column id="card" surface="card"><Text id="value" text={item.label} variant="metric" tone={item.tone}/></Column></For>'));
 const a={id:'a',label:'42%',tone:'warning'},b={id:'b',label:'10%',tone:'neutral'};
 const rows=items=>resolve(tree,{items}).children[0].children[0];
 assert.equal(rows([a,b]).type,'Grid');assert.equal(rows([a,b]).props.columns,2);
 assert.equal(rows([a,b]).children[0].id,rows([b,a]).children[1].id);
 assert.throws(()=>rows([{...a,tone:'red'}]),/expected tone/);
 for(const source of ['<Text id="t" text="x" variant="giant"/>','<Column id="c" surface="html"/>','<For id="f" items={state.items} key={item.id} columns={1.5}><Text id="t" text={item.label}/></For>'])assert.throws(()=>resolve(compile(wrap(source)),{items:[a]}));
});
test('chart reference ranges are explicit and bounded',()=>{
 const chart='<Chart id="chart" values={state.values} label="CPU" min={0} max={100} unit="%" threshold={90}/>';
 const tree=compile(wrap(chart));
 const node=resolve(tree,{values:[0,25,null,95]}).children[0].children[0];
 assert.equal(node.props.max,100);assert.equal(node.props.threshold,90);assert.deepEqual(node.props.values,[0,25,null,95]);
 for(const invalid of [chart.replace(' max={100}',''),chart.replace('max={100}','max={0}'),chart.replace('threshold={90}','threshold={110}')])assert.throws(()=>compile(wrap(invalid)),/chart range|chart threshold/);
});
