"""Generate Talìa's editable SVG concept gallery using only Python's stdlib."""
from pathlib import Path
import html

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'assets/icon-concepts/round-3'
OUT.mkdir(parents=True, exist_ok=True)
L, M, B = '#bfdbfe', '#60a5fa', '#2563eb'

def path(d, c, stroke=False, w=8):
    return f'<path d="{d}" fill="{"none" if stroke else c}"' + (f' stroke="{c}" stroke-width="{w}" stroke-linecap="round" stroke-linejoin="round"' if stroke else '') + '/>'
def rect(x,y,w,h,c,r=6):
    return f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{r}" fill="{c}"/>'
def circle(x,y,r,c):
    return f'<circle cx="{x}" cy="{y}" r="{r}" fill="{c}"/>'
def ellipse(x,y,rx,ry,c):
    return f'<ellipse cx="{x}" cy="{y}" rx="{rx}" ry="{ry}" fill="{c}"/>'
def line(d,c,w=8): return path(d,c,True,w)
def svg(body,tile=False):
    return '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" fill="none">'+(rect(0,0,100,100,'#fff',22) if tile else '')+body+'</svg>'
def lids(top=17,bottom=83,w=12):
    return line(f'M15 43Q50 {top} 85 43',L,w)+line(f'M15 57Q50 {bottom} 85 57',M,w)
def bezel():
    return path('M23 17H77Q86 17 86 26V65Q86 73 77 73H23Q14 73 14 65V26Q14 17 23 17ZM24 27V62H76V27Z',L)
def stand():return rect(43,72,14,10,M,3)+rect(31,83,38,7,B,3.5)
def bell(c=L):return path('M24 66Q29 60 29 42Q29 23 45 20V16Q45 11 50 11Q55 11 55 16V20Q71 23 71 42Q71 60 76 66Q79 72 72 72H28Q21 72 24 66Z',c)
items=[]
def add(group,name,body):
    number=1+sum(i['group']==group for i in items)
    code=f"{dict(Eye='E01',Monitor='M03',Dashboard='D01',Alarm='A09')[group]}-{number:02}"
    items.append(dict(group=group,name=name,body=body,code=code))

# Refinements of the four selected round-2 silhouettes.
for name,top,bottom,width,radius,px in [
    ('Selected eye',6,94,13,15,50),('Airy lids',6,94,10,14,50),
    ('Bold lids',6,94,16,14,50),('Wide gaze',17,83,12,12,50),
    ('Tall gaze',-3,103,13,15,50),('Small pupil',6,94,13,11,50),
    ('Full pupil',0,100,12,18,50),('Soft glance',6,94,13,14,57),
    ('Rightward',6,94,13,14,62),('Fine focus',0,100,9,14,50),
    ('Heavy lower lid',6,94,13,15,50),('Heavy upper lid',6,94,13,15,50)]:
    body=lids(top,bottom,width)
    if name=='Heavy lower lid': body=line('M15 43Q50 6 85 43',L,9)+line('M15 57Q50 94 85 57',M,17)
    if name=='Heavy upper lid': body=line('M15 43Q50 6 85 43',L,17)+line('M15 57Q50 94 85 57',M,9)
    add('Eye',name,body+circle(px,50,radius,B))

def frame(x,y,w,h,r,thick):
    return f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{r}" fill="none" stroke="{L}" stroke-width="{thick}"/>'
def bars(xs,base,heights,width=10,r=3):
    return ''.join(rect(x,base-h,width,h,M if n<2 else B,r) for n,(x,h) in enumerate(zip(xs,heights)))
# None of these variants has a monitor stand or base.
add('Monitor','Selected, no stand','<g transform="translate(0 5)">'+bezel()+bars([29,45,61],59,[12,21,29],9)+'</g>')
add('Monitor','Soft square',frame(17,17,66,66,11,9)+bars([29,45,61],69,[18,29,41],10,4))
add('Monitor','Wide frame',frame(12,25,76,50,9,8)+bars([28,45,62],65,[11,20,30],10,4))
add('Monitor','Fine frame',frame(16,21,68,58,9,5)+bars([29,45,61],67,[15,26,38],10,4))
add('Monitor','Bold frame',frame(16,21,68,58,10,12)+bars([29,45,61],65,[12,22,32],10,4))
add('Monitor','Pill bars',frame(15,20,70,60,13,8)+bars([28,44,60],68,[17,29,40],12,6))
add('Monitor','Narrow bars',frame(16,21,68,58,9,8)+bars([31,47,63],67,[15,26,38],7,3.5))
add('Monitor','Even climb',frame(16,21,68,58,9,8)+bars([28,44,60],67,[16,24,32],12,4))
add('Monitor','Strong finish',frame(16,21,68,58,9,8)+bars([29,45,61],67,[10,18,39],10,4))
add('Monitor','Open corners',line('M15 44V28Q15 21 23 21H77Q85 21 85 28V44',L,9)+line('M15 57V72Q15 79 23 79H77Q85 79 85 72V57',L,9)+bars([29,45,61],65,[14,24,34],10,4))
add('Monitor','Inset bars',frame(16,21,68,58,9,8)+bars([33,46,59],63,[10,18,26],8,3))
add('Monitor','Tall instrument',frame(21,14,58,72,11,8)+bars([31,45,59],74,[22,36,49],10,4))

for name,left,gap,split,rad in [
    ('Selected panels',23,6,27,8),('Slim rail',17,6,27,8),
    ('Broad rail',30,6,27,8),('Even split',23,6,30,8),
    ('Large top',23,6,39,8),('Large bottom',23,6,20,8),
    ('Close seams',23,3,28,8),('Open seams',23,10,25,8),
    ('Rounder panels',23,6,27,12),('Crisp panels',23,6,27,4),
    ('Pill rail',16,8,27,8),('Stepped cards',23,6,27,8)]:
    x=16+left+gap;w=84-x
    body=rect(16,17,left,66,L,left/2 if name=='Pill rail' else rad)
    body+=rect(x,17,w,split,M,rad)
    body+=rect(x,17+split+gap,w-(5 if name=='Stepped cards' else 0),66-split-gap,B,rad)
    add('Dashboard',name,body)

# Alarm refinement: one filled silhouette, optionally one substantial clapper.
# No hanging loop, ringing strokes, separate rim, or tiny cut lines.
def notice_with_bell(shape):
    card=path('M27 29H55V37Q55 48 66 48H81V69Q81 81 69 81H27Q15 81 15 69V41Q15 29 27 29Z',L)
    card+=rect(25,43,23,7,M,3.5)+rect(25,58,25,7,M,3.5)
    return card+f'<g transform="translate(60 12)">{shape}</g>'

simple_bells=[
    ('Soft dome',path('M2 29V15Q2 1 16 1Q30 1 30 15V29Q30 32 27 32H5Q2 32 2 29Z',B)),
    ('Flared dome',path('M7 14Q7 2 16 2Q25 2 25 14V20L31 28Q33 32 28 32H4Q-1 32 1 28L7 20Z',B)),
    ('Wide dome',path('M1 28V16Q1 3 16 3Q31 3 31 16V28Q31 31 28 31H4Q1 31 1 28Z',B)),
    ('Tall dome',path('M5 30V13Q5 0 16 0Q27 0 27 13V30Q27 33 24 33H8Q5 33 5 30Z',B)),
    ('Soft flare',path('M7 13Q7 2 16 2Q25 2 25 13Q25 23 30 27Q34 32 28 32H4Q-2 32 2 27Q7 23 7 13Z',B)),
    ('One-piece bell',path('M6 13Q6 1 16 1Q26 1 26 13V21L31 28Q33 31 29 31H22Q22 37 16 37Q10 37 10 31H3Q-1 31 1 28L6 21Z',B)),
    ('Dome and dot',path('M3 25V13Q3 0 16 0Q29 0 29 13V25Q29 28 26 28H6Q3 28 3 25Z',B)+circle(16,35,4,B)),
    ('Flare and dot',path('M7 12Q7 0 16 0Q25 0 25 12V18L31 25Q34 28 29 28H3Q-2 28 1 25L7 18Z',B)+circle(16,35,4,B)),
    ('Dome and chime',path('M2 24V14Q2 0 16 0Q30 0 30 14V24Q30 27 27 27H5Q2 27 2 24Z',B)+path('M10 33H22Q22 39 16 39Q10 39 10 33Z',B)),
    ('Flare and chime',path('M7 12Q7 0 16 0Q25 0 25 12Q25 20 30 24Q34 28 29 28H3Q-2 28 2 24Q7 20 7 12Z',B)+path('M10 33H22Q22 39 16 39Q10 39 10 33Z',B)),
    ('Compact bell',path('M4 25V14Q4 2 16 2Q28 2 28 14V25Q28 28 25 28H7Q4 28 4 25Z',B)+rect(11,33,10,6,B,3)),
    ('Broad bell',path('M5 13Q5 1 16 1Q27 1 27 13V19L33 25Q36 28 31 28H1Q-4 28 -1 25L5 19Z',B)+rect(10,33,12,6,B,3)),
]
for name,shape in simple_bells:
    add('Alarm',name,notice_with_bell(shape))

# Minimal exclamation alternatives: only a stem and a dot.
for name,x,y,w,h,radius,gap,dot in [
    ('Exclamation',11,1,10,22,5,5,5),
    ('Bold exclamation',9,0,14,23,6,5,6),
    ('Tall exclamation',11,-3,10,26,5,5,5),
    ('Compact exclamation',10,4,12,18,5,5,5),
    ('Square exclamation',10,1,12,22,3,6,5),
    ('Tapered exclamation',10,0,12,24,4,5,5),
]:
    stem=rect(x,y,w,h,B,radius)
    if name=='Tapered exclamation':
        stem=path('M12 0H20Q23 0 22 4L20 22Q20 25 16 25Q12 25 12 22L10 4Q9 0 12 0Z',B)
    mark=stem+(rect(11,30,10,9,B,2) if name=='Square exclamation' else circle(16,y+h+gap+dot,dot,B))
    add('Alarm',name,notice_with_bell(mark))

for i in items:
    for tile in (False,True):
        (OUT/f"{i['code']}{'-tile' if tile else ''}.svg").write_text(svg(i['body'],tile)+'\n')

family=[]
for name,filename in [('Crumbles','../crumbles/crumbles-web/public/favicon.svg'),('Simple Agents','../simple-agents/web/brand.svg'),('ScT','../sct/web/public/brand.svg')]:
    family.append(f'<div class="family-item">{(ROOT/filename).read_text()}<span>{name}</span></div>')
cards=[]
for i in items:
    code=i['code']; body=i['body']
    sizes=''.join(f'<span>{svg(body,True)}<small>{z}px</small></span>' for z in (16,24,32,48))
    cards.append(f'''<article class="card" data-group="{i['group']}" data-code="{code}" id="{code}"><div class="stage">{svg(body,True)}<button class="star" aria-label="Shortlist {code}" aria-pressed="false" title="Shortlist this option">☆</button></div><div class="info"><p class="code">{i['group'].upper()} / {code}</p><h2>{i['name']}{' · Selected' if code == 'E01-04' else ''}</h2><div class="sizes">{sizes}</div><div class="links"><a href="../assets/icon-concepts/round-3/{code}.svg">Mark SVG</a><a href="../assets/icon-concepts/round-3/{code}-tile.svg">Tile SVG</a></div></div></article>''')
page='''<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Talìa · Four directions</title><style>
*{box-sizing:border-box}body{margin:0;background:#f9fafb;color:#111827;font:14px/1.5 system-ui,sans-serif}.shell{max-width:1440px;margin:auto;padding:44px 28px}header{display:flex;align-items:center;justify-content:space-between;gap:32px}h1{font-size:clamp(36px,5vw,60px);letter-spacing:-2px;line-height:1.1;margin:12px 0 18px;font-weight:650}h1 span{color:#9ca3af}.intro{color:#4b5563;max-width:660px;font-size:16px}.eyebrow,.code{font-size:11px;letter-spacing:1.6px;color:#2563eb;font-weight:750;margin:0}.family{display:flex;gap:20px}.family-item{display:flex;align-items:center;flex-direction:column;gap:8px;color:#6b7280;white-space:nowrap;font-size:11px}.family svg{width:60px;height:60px}.toolbar{position:sticky;top:0;z-index:3;background:#f9fafbf5;backdrop-filter:blur(12px);padding:18px 0;display:flex;justify-content:space-between;gap:12px;flex-wrap:wrap;border-block:1px solid #e5e7eb;margin:30px 0 22px}.filters,.settings{display:flex;gap:8px;flex-wrap:wrap}button{font:inherit;cursor:pointer;background:#fff;border:1px solid #d1d5db;border-radius:9px;padding:8px 12px;color:#374151}button[aria-pressed="true"]{background:#2563eb;color:#fff;border-color:#2563eb}button:focus-visible,a:focus-visible{outline:3px solid #60a5fa;outline-offset:4px}.status{margin:0 0 20px;color:#6b7280}.grid{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));gap:18px}.card{border:1px solid #e5e7eb;border-radius:18px;overflow:hidden;background:#fff}.card[hidden]{display:none}.stage{height:190px;display:grid;place-items:center;background:#eff6ff;position:relative}.stage>svg{height:128px;width:128px}.star{position:absolute;top:10px;right:10px;font-size:23px;line-height:1;padding:6px 9px;border:none;background:#ffffffc9}.info{padding:19px}h2{margin:5px 0 0;font-size:20px;letter-spacing:-.4px}.sizes{display:flex;gap:22px;align-items:end;height:82px}.sizes span{display:flex;flex-direction:column;align-items:center;gap:6px}.sizes small{font-size:10px;color:#9ca3af}.sizes span:nth-child(1) svg{width:16px;height:16px}.sizes span:nth-child(2) svg{width:24px;height:24px}.sizes span:nth-child(3) svg{width:32px;height:32px}.sizes span:nth-child(4) svg{width:48px;height:48px}.links{display:flex;gap:18px;border-top:1px solid #f3f4f6;margin-top:16px;padding-top:12px}a{color:#2563eb;text-decoration:none;font-size:12px}a:hover{text-decoration:underline}body.dark .stage{background:#111827}body.bare .stage>svg>rect:first-child{display:none}.foot{margin-top:32px;color:#6b7280;font-size:12px}.empty{padding:50px;text-align:center;color:#6b7280} @media(max-width:1100px){.grid{grid-template-columns:repeat(3,minmax(0,1fr))}header{display:block}.family{margin-top:22px;justify-content:start}}@media(max-width:800px){.grid{grid-template-columns:repeat(2,minmax(0,1fr))}.shell{padding:25px 18px}.stage{height:165px}}@media(max-width:480px){.grid{grid-template-columns:1fr}.toolbar{position:static}.family{gap:16px}}
</style></head><body><main class="shell"><header><div><p class="eyebrow">TALÌA / IDENTITY STUDIES / ROUND 03</p><h1>Four picks.<br><span>A closer look.</span></h1><p class="intro">Refining E01, M03 without its stand, D01, and A09 with an alarm instead of the dot. E01, M03 and D01 have twelve variations each. A09 compares twelve simplified bells with six minimal exclamation marks.</p></div><div class="family">FAMILY</div></header><nav class="toolbar" aria-label="Gallery controls"><div class="filters">FILTERS<button data-filter="Shortlist" aria-pressed="false">★ Shortlist</button></div><div class="settings"><button id="dark" aria-pressed="false">Dark surroundings</button><button id="bare" aria-pressed="false">Transparent marks</button></div></nav><p class="status" role="status" id="status"></p><section class="grid" aria-label="Icon options">CARDS</section><p class="empty" id="empty" hidden>Star some options to build your shortlist.</p><p class="foot">Current selection: E01-04 · Wide gaze. Other options remain exploration. Palette: #bfdbfe · #60a5fa · #2563eb. Small previews are actual CSS sizes. Shortlist is saved in this browser when storage is available.<br><a href="../assets/icon-concepts/round-3/eye.png">Eye sheet</a> · <a href="../assets/icon-concepts/round-3/monitor.png">Monitor sheet</a> · <a href="../assets/icon-concepts/round-3/dashboard.png">Dashboard sheet</a> · <a href="../assets/icon-concepts/round-3/alarm.png">Alarm sheet</a></p></main><script>
let selected=new Set();try{const saved=JSON.parse(localStorage.getItem('talia-icons-r3')||'[]');if(Array.isArray(saved))selected=new Set(saved);}catch{}let filter=location.hash==='#Alarm'?'Alarm':'All';const cards=[...document.querySelectorAll('.card')];
function render(){let count=0;cards.forEach(c=>{const saved=selected.has(c.dataset.code);c.hidden=!(filter==='All'||filter===c.dataset.group||filter==='Shortlist'&&saved);if(!c.hidden)count++;const star=c.querySelector('.star');star.textContent=saved?'★':'☆';star.setAttribute('aria-pressed',String(saved));});document.querySelectorAll('[data-filter]').forEach(b=>b.setAttribute('aria-pressed',String(b.dataset.filter===filter)));document.getElementById('status').textContent=`${filter} · ${count} options · ${selected.size} shortlisted${selected.size?' · '+[...selected].sort().join(', '):''}`;document.getElementById('empty').hidden=count!==0;}
document.querySelectorAll('[data-filter]').forEach(b=>b.addEventListener('click',()=>{filter=b.dataset.filter;render();}));cards.forEach(c=>c.querySelector('.star').addEventListener('click',()=>{const id=c.dataset.code;selected.has(id)?selected.delete(id):selected.add(id);try{localStorage.setItem('talia-icons-r3',JSON.stringify([...selected]));}catch{}render();}));['dark','bare'].forEach(id=>document.getElementById(id).addEventListener('click',function(){this.setAttribute('aria-pressed',String(document.body.classList.toggle(id)));}));render();
</script></body></html>'''
filters=''.join(f'<button data-filter="{g}" aria-pressed="{"true" if g=="All" else "false"}">{g}{(" · 18" if g=="Alarm" else " · 12") if g!="All" else " · 54"}</button>' for g in ['All','Eye','Monitor','Dashboard','Alarm'])
(ROOT/'docs/icon-concepts.html').write_text(page.replace('FAMILY</div>', ''.join(family)+'</div>').replace('FILTERS',filters).replace('CARDS</section>',''.join(cards)+'</section>'))
for group in ['Eye','Monitor','Dashboard','Alarm']:
    group_count=sum(i['group']==group for i in items)
    sheet_height=120+((group_count+3)//4)*296+52
    pieces=[f'<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="{sheet_height}" viewBox="0 0 1200 {sheet_height}" fill="none"><rect width="1200" height="{sheet_height}" fill="#f9fafb"/>',f'<text x="40" y="56" font-family="DejaVu Sans" font-size="30" fill="#111827">Talìa / {group}</text><text x="40" y="88" font-family="DejaVu Sans" font-size="14" fill="#6b7280">Round 03 · {group_count} alternatives · Draft concepts</text>']
    for n,i in enumerate(j for j in items if j['group']==group):
        x=40+n%4*288;y=120+n//4*296
        pieces.append(f'<rect x="{x}" y="{y}" width="264" height="274" rx="18" fill="white" stroke="#e5e7eb"/><svg x="{x+68}" y="{y+22}" width="128" height="128" viewBox="0 0 100 100">{i["body"]}</svg><text x="{x+18}" y="{y+185}" font-family="DejaVu Sans" font-size="17" fill="#111827">{i["code"]} · {html.escape(i["name"])}</text>')
        for j,z in enumerate((16,24,32,48)):
            pieces.append(f'<svg x="{x+20+j*57}" y="{y+210}" width="{z}" height="{z}" viewBox="0 0 100 100">{i["body"]}</svg>')
    pieces.append('</svg>')
    (OUT/f'{group.lower()}.svg').write_text(''.join(pieces))
print(f'Generated {len(items)} concepts and four contact sheets.')
