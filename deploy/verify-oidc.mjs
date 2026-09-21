// Live qualification with a temporary, non-admin account authorized only for Talìa.
// Credentials are private files; no tokens, passwords or callback URLs are printed.
import fs from 'node:fs';import {execFileSync} from 'node:child_process';
import {createRequire} from 'node:module';const require=createRequire(import.meta.url);
const {chromium}=require('../spikes/runtime/node_modules/playwright');
const origin='https://talia.lan.lelloman.com',issuer='https://auth.lelloman.com';
const user=JSON.parse(fs.readFileSync('.local/oidc/verification-user.json','utf8'));
if(!/^[0-9a-f-]{36}$/.test(user.id))throw Error('invalid verification identity');
const checks=[];let phase='launch';const browser=await chromium.launch({headless:true,args:['--no-sandbox']});
try{
 const context=await browser.newContext(),page=await context.newPage();
 async function signIn(){
  await page.goto(origin);await page.getByRole('link',{name:'Sign in with LelloAuth'}).click();
  const deadline=Date.now()+40000;let submitted=false;
  while(Date.now()<deadline){
   if(page.url().startsWith(origin)&&await page.getByText('Talìa is online',{exact:true}).isVisible())return;
   if(new URL(page.url()).origin===issuer){
    if(!submitted&&await page.locator('input[name=username]').isVisible()){
     await page.locator('input[name=username]').fill(user.username);await page.locator('input[name=password]').fill(user.password);await page.locator('button[type=submit]').click();submitted=true;
    }else if(await page.locator('button[value=allow]').isVisible())await page.locator('button[value=allow]').click();
   }
   await page.waitForTimeout(100);
  }
  throw Error('Sign-in did not complete');
 }
 phase='old key rejection';const old='a'.repeat(64);
 let r=await context.request.post(origin+'/engine',{headers:{Origin:origin,'X-Talia-Access':old,Cookie:'__Host-talia='+old},data:{}});if(r.status()!==401)throw Error('old key path accepted or wrong rejection');
 r=await context.request.post(origin+'/session',{headers:{Origin:origin},data:{token:old}});if(![404,405].includes(r.status()))throw Error('old session endpoint exists');checks.push('shared-key cookie/header/login endpoint removed');
 phase='LelloAuth login';await signIn();await page.waitForFunction(()=>window.dashboardReport?.registration?.clientId);
 let identity=await (await context.request.get(origin+'/auth/session')).json();if(!identity.subject.endsWith('#'+user.id))throw Error('wrong identity');checks.push('real LelloAuth authorization-code login, consent, callback and subject');
 const client=await page.evaluate(()=>window.dashboardReport.registration.clientId);
 phase='alerts';await page.locator('#alerts-panel summary').click();await page.getByText('No alerts',{exact:true}).waitFor();if(await page.locator('#alert-access').isVisible())throw Error('alert key prompt remains');checks.push('browser alerts use OIDC identity without API-key entry');
 phase='restart';execFileSync('ssh',['homelab','docker compose -f /home/lelloman/homelab/talia/docker-compose.yml restart talia'],{stdio:'pipe'});
 const readyUntil=Date.now()+30000;while(Date.now()<readyUntil){try{if((await context.request.get(origin+'/healthz')).ok())break;}catch{}await page.waitForTimeout(500);}
 await page.reload();await page.getByText('Talìa is online',{exact:true}).waitFor({timeout:30000});await page.waitForFunction(id=>window.dashboardReport?.registration?.clientId===id,client);checks.push('OIDC session and client registration survive service restart');
 phase='logout';const saved=await context.cookies(origin);await page.getByRole('button',{name:'Sign out',exact:true}).click();await page.getByRole('link',{name:'Sign in with LelloAuth'}).waitFor();
 const stale=await browser.newContext();await stale.addCookies(saved);if((await stale.request.get(origin+'/auth/session')).status()!==401)throw Error('logged-out cookie replay accepted');await stale.close();checks.push('logout invalidates the server session and old cookie replay');
 phase='SSO sign-in';await signIn();checks.push('existing LelloAuth SSO can sign back in');
 phase='revocation';
 const sql=`DELETE FROM user_app_access WHERE user_id='${user.id}' AND app_id='talia';`;
 execFileSync('ssh',['homelab',"docker run --rm -i --network none --user 0 --entrypoint sqlite3 --mount type=bind,src=/home/lelloman/homelab-data/lelloauth,dst=/auth registry.homelab:5000/talia:latest /auth/lelloauth.db"],{input:sql,stdio:['pipe','pipe','pipe']});
 const deadline=Date.now()+45000;let revoked=false;while(Date.now()<deadline){if((await context.request.get(origin+'/auth/session')).status()===401){revoked=true;break;}await page.waitForTimeout(1000);}if(!revoked)throw Error('revoked app access remains valid');checks.push('revoked LelloAuth app access invalidates a live browser session');
 const report={passed:true,checkedAt:new Date().toISOString(),origin,issuer,checks};fs.writeFileSync('.local/oidc/verification.json',JSON.stringify(report,null,2)+'\n',{mode:0o600});console.log(JSON.stringify(report));
}catch(e){console.error('OIDC verification failed during '+phase+': '+e.name);process.exitCode=1;}finally{await browser.close();}
