#!/usr/bin/env python3
"""HTTP validation and concurrent idempotency checks against the running loopback server."""
import concurrent.futures, json, sys, time, urllib.request, urllib.error, uuid
port=int(sys.argv[1]); namespace='validation-'+uuid.uuid4().hex
checks=[]
def call(op,args,**overrides):
    data=dict(session=namespace,channel='test',epoch=1,id=1,op=op,args=args)
    data.update(overrides)
    request=urllib.request.Request(f'http://127.0.0.1:{port}/rpc',data=json.dumps(data).encode(),headers={'Content-Type':'application/json'})
    with urllib.request.urlopen(request,timeout=8) as response:return json.load(response)
def check(ok,name):
    assert ok,name
    checks.append(name)
def bad_status(data,expected):
    try:
        urllib.request.urlopen(urllib.request.Request(f'http://127.0.0.1:{port}/rpc',data=data,headers={'Content-Type':'application/json'}),timeout=3)
    except urllib.error.HTTPError as e:return e.code==expected
    return False
check(bad_status(b'null',422),'non-object envelope rejected')
check(bad_status(b'x'*32769,413),'oversized body rejected')
check('error' in call('action',{'actionId':'range','value':-9223372036854775808}),'out-of-range mutation rejected without panic')
check('error' in call('action',{'actionId':'extra','value':1,'extra':True}),'unknown action fields rejected')
try:
    call('read',{'tag':'read'},session='../invalid')
    raise AssertionError('invalid namespace accepted')
except urllib.error.HTTPError as e:check(e.code==400,'invalid namespace rejected')
call('test',{'command':'hold','key':'concurrent'})
with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
    futures=[pool.submit(call,'action',{'actionId':'concurrent','value':7}) for _ in range(2)]
    for _ in range(100):
        if call('status',{'actionId':'concurrent'})['value']['status']=='accepted':break
        time.sleep(.005)
    else:raise AssertionError('action not admitted')
    check(call('read',{'tag':'parallel'})['value']['value']==0,'read progresses while server action awaits')
    call('test',{'command':'release','key':'concurrent'})
    replies=[f.result()['value'] for f in futures]
check(all(r['status']=='completed' for r in replies) and call('read',{'tag':'after'})['value']['revision']==1,
      'concurrent duplicate actions share one effect')
check('error' in call('action',{'actionId':'concurrent','value':8}),'conflicting idempotency payload rejected')
call('test',{'command':'hold','key':'expires'})
expired=call('action',{'actionId':'expires','value':99})['value']
check(expired['status']=='failed' and call('read',{'tag':'failed'})['value']['value']==7,
      'pre-effect timeout records failure without changing state')
call('test',{'command':'release','key':'expires'})
check(call('action',{'actionId':'expires','value':99})['value']['status']=='failed',
      'retry cannot revive an action recorded as failed')
print(json.dumps({'passed':True,'checks':checks}))
