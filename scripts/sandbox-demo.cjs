// Reproducible end-to-end demo against PayPal Sandbox through the agent console.
// Story 1: injected delivery -> policy hold -> reject -> PayPal void.
// Story 2: compliant delivery -> buyer approval -> PayPal capture.
// Requires: server on http://localhost:3000 with QP_MODE=sandbox, the local model running, and
// Playwright (`npm i playwright@1.49` in any scratch directory, then NODE_PATH=<dir>/node_modules).
// QP_SANDBOX_ACCOUNTS points to the PayPal Sandbox accounts CSV export (default
// .local/quarantinepay-sandbox-accounts-fixed.csv); only the PERSONAL test buyer is used.
// Usage: [QP_DEMO_BASE=https://your-host] node scripts/sandbox-demo.cjs <out-dir>
const {chromium}=require('playwright');const fs=require('fs');const path=require('path');
const root=path.resolve(__dirname,'..');
const BASE=(process.env.QP_DEMO_BASE||'http://localhost:3000').replace(/\/$/,'');const BASE_HOST=new URL(BASE).hostname;const out=process.argv[2]||'/tmp/qp-demo';fs.mkdirSync(out,{recursive:true});
const env=fs.readFileSync(root+'/.env','utf8');const pwm=env.match(/^QP_ADMIN_PASSWORD=(.+)$/m);
const pw=pwm&&pwm[1].trim()?pwm[1].trim():fs.readFileSync(root+'/.local/admin-password').toString('hex');
const csv=fs.readFileSync(process.env.QP_SANDBOX_ACCOUNTS||root+'/.local/quarantinepay-sandbox-accounts-fixed.csv','utf8').trim().split('\n').map(x=>x.split(','));const buyer=csv.slice(1).find(x=>x[1]==='PERSONAL');
const REQUEST='Find me a market research report on AI note-taking apps under $100 covering three competitors, including credible sources. Negotiate the best offer, establish delivery requirements, and prepare the purchase. Only release the payment if the agreed requirements are satisfied.';
const api=async p=>(await fetch(BASE+p,{headers:{Authorization:'Bearer '+pw}})).json();
const t0=Date.now();const log=(...a)=>console.log(`[${((Date.now()-t0)/1000).toFixed(0).padStart(3)}s]`,...a);
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function waitTask(id,pred,ms=180000){const end=Date.now()+ms;while(Date.now()<end){const t=await api('/api/agent/tasks/'+id);if(pred(t))return t;await sleep(1000);}throw new Error('task wait timeout');}
async function waitOrder(id,states,ms=180000){const end=Date.now()+ms;while(Date.now()<end){const o=await api('/api/orders/'+id);if(states.includes(o.state))return o;await sleep(1000);}throw new Error('order wait timeout '+states);}
(async()=>{
 const browser=await chromium.launch({headless:true,args:['--no-sandbox']});
 const ctx=await browser.newContext({viewport:{width:1440,height:900},recordVideo:{dir:out,size:{width:1440,height:900}}});
 const page=await ctx.newPage();const evidence=[];
 const btn=n=>page.getByRole('button',{name:n,exact:false}).first();
 try{
  await page.goto(BASE+'/agent');await page.locator('input[name=password]').fill(pw);await page.getByRole('button',{name:'Sign in'}).click();
  // Sign the Sandbox buyer in once, in its own tab.
  const pp=await ctx.newPage();await pp.goto('https://www.sandbox.paypal.com/signin',{waitUntil:'domcontentloaded',timeout:45000});
  await pp.locator('#email').fill(buyer[2],{timeout:15000});if(await pp.locator('#btnNext').isVisible())await pp.locator('#btnNext').click();
  await pp.locator('#password').fill(buyer[3],{timeout:15000});await pp.locator('#btnLogin').click();await pp.waitForTimeout(4000);
  for(const story of ['malicious','pass']){
   log(story,': buyer request');await page.bringToFront();await page.goto(BASE+'/agent');
   await page.locator('textarea[name=request]').fill(REQUEST);await btn('Run buyer agent').click();await page.waitForURL(/\/agent\/[0-9a-f-]{36}$/);
   const taskUrl=page.url();const taskId=taskUrl.split('/').pop();
   const task=await waitTask(taskId,t=>!['planning','negotiating'].includes(t.state));if(task.state!=='awaiting_approval')throw new Error('no proposal: '+task.state);
   log(story,': proposal',task.proposal.seller,task.proposal.amount_minor/100,'planner',task.planner.source);
   await page.goto(taskUrl);await page.screenshot({path:`${out}/${story}-1-proposal.png`,fullPage:true});
   await btn('Approve exact contract').click();
   const t2=await waitTask(taskId,t=>t.order&&t.order.state==='approval_pending',60000);const order=t2.order;
   const u=new URL(order.approval_url);if(u.hostname!=='www.sandbox.paypal.com'&&!u.hostname.endsWith('.sandbox.paypal.com'))throw new Error('bad host');
   log(story,': PayPal order',order.paypal_order);await page.goto(taskUrl);
   await pp.bringToFront();await pp.goto(order.approval_url,{waitUntil:'domcontentloaded',timeout:45000});await pp.waitForTimeout(3000);
   for(let i=0;i<8;i++){if(new URL(pp.url()).hostname===BASE_HOST)break;const b=pp.getByRole('button',{name:/^(Pay Now|Agree & Pay|Agree and Pay|Complete Purchase|Continue|Agree & Continue|Agree and Continue|Continue to Review Order|Pay)$/i}).first();if(await b.isVisible().catch(()=>false))await b.click();await pp.waitForTimeout(3000);}
   if(new URL(pp.url()).hostname!==BASE_HOST)throw new Error('PayPal approval did not return');
   await page.bringToFront();await page.goto(taskUrl);await btn('I approved').click();
   const auth=await waitOrder(order.id,['authorized','reconciliation_required'],60000);if(auth.state!=='authorized')throw new Error('authorization '+auth.state);
   log(story,': authorized',auth.authorization_id);await page.goto(taskUrl);await page.screenshot({path:`${out}/${story}-2-authorized.png`,fullPage:true});
   await btn(story==='pass'?'Deliver compliant report':'Deliver report with injected instructions').click();
   const rev=await waitOrder(order.id,['buyer_review','voided','void_pending'],150000);
   log(story,': analysis',rev.analysis&&rev.analysis.risk,rev.analysis&&rev.analysis.recommended_action,'hold',rev.analysis&&rev.analysis.policy_hold);
   await page.goto(taskUrl);await page.locator('.analysis, [class*=analyst]').first().scrollIntoViewIfNeeded().catch(()=>{});
   await page.screenshot({path:`${out}/${story}-3-review.png`,fullPage:true});
   await btn(story==='pass'?'Approve delivery':'Reject delivery').click();
   const done=await waitOrder(order.id,['captured','voided','reconciliation_required'],90000);
   log(story,': final',done.state,JSON.stringify(done.provider_result&&{id:done.provider_result.id,status:done.provider_result.status}));
   await page.goto(taskUrl);await page.waitForTimeout(800);await page.screenshot({path:`${out}/${story}-4-final.png`,fullPage:true});
   evidence.push({story,task:taskId,order:order.id,paypal_order:order.paypal_order,authorization:auth.authorization_id,amount:task.proposal.amount_minor,planner:task.planner.source,analysis:{risk:rev.analysis&&rev.analysis.risk,action:rev.analysis&&rev.analysis.recommended_action,policy_hold:rev.analysis&&rev.analysis.policy_hold,analyst:rev.analysis&&rev.analysis.analyst.source},final_state:done.state,provider_result:done.provider_result&&{id:done.provider_result.id,status:done.provider_result.status}});
  }
  await page.goto(BASE+'/intelligence');await page.waitForTimeout(4000);await page.screenshot({path:`${out}/intelligence.png`,fullPage:true});
  fs.writeFileSync(`${out}/evidence.json`,JSON.stringify(evidence,null,1));log('complete');
 }catch(e){log('stopped:',e.message.split('\n')[0]);await page.screenshot({path:`${out}/error.png`,fullPage:true}).catch(()=>{});process.exitCode=1;}
 finally{await ctx.close();await browser.close();}
})();
