// Records the narrated demo: each narration line gets its own short browser recording (so cuts
// never drift), while an unrecorded driver context and a PayPal Sandbox buyer context do the waiting.
// Usage: node scripts/record-demo.cjs <out-dir> '<json array of narration durations in seconds>'
// Requirements as for scripts/sandbox-demo.cjs. The output shots.json lists each shot's file and
// trim points; demo/quarantinepay-agent-demo.mp4 was assembled from them with ffmpeg.
const {chromium}=require('playwright');const fs=require('fs');const path=require('path');
const root=path.resolve(__dirname,'..');const out=process.argv[2];const durs=JSON.parse(process.argv[3]);fs.mkdirSync(out,{recursive:true});
const env=fs.readFileSync(root+'/.env','utf8');const pwm=env.match(/^QP_ADMIN_PASSWORD=(.+)$/m);
const pw=pwm&&pwm[1].trim()?pwm[1].trim():fs.readFileSync(root+'/.local/admin-password').toString('hex');
const csv=fs.readFileSync(process.env.QP_SANDBOX_ACCOUNTS||root+'/.local/quarantinepay-sandbox-accounts-fixed.csv','utf8').trim().split('\n').map(x=>x.split(','));const buyer=csv.slice(1).find(x=>x[1]==='PERSONAL');
const REQUEST='Find me a market research report on AI note-taking apps under $100 covering three competitors, including credible sources. Negotiate the best offer and only release the payment if the agreed requirements are satisfied.';
const H={Authorization:'Bearer '+pw,'Content-Type':'application/json'};
const api=async(p,body)=>(await fetch('http://localhost:3000'+p,body?{method:'POST',headers:H,body:JSON.stringify(body)}:{headers:H})).json();
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function waitTask(id,pred,ms=180000){const end=Date.now()+ms;while(Date.now()<end){const t=await api('/api/agent/tasks/'+id);if(pred(t))return t;await sleep(800);}throw new Error('task timeout');}
async function waitOrder(id,states,ms=180000){const end=Date.now()+ms;while(Date.now()<end){const o=await api('/api/orders/'+id);if(states.includes(o.state))return o;await sleep(800);}throw new Error('order timeout '+states);}
const D=i=>Math.round((durs[i]+0.9)*1000);
(async()=>{
 const browser=await chromium.launch({headless:true,args:['--no-sandbox']});
 const driver=await browser.newContext({viewport:{width:1280,height:800}});const dp=await driver.newPage();
 await dp.goto('http://localhost:3000/agent');await dp.locator('input[name=password]').fill(pw);await dp.getByRole('button',{name:'Sign in'}).click();await dp.waitForTimeout(800);
 const state=await driver.storageState();
 const ppc=await browser.newContext({viewport:{width:1280,height:900}});const pp=await ppc.newPage();
 await pp.goto('https://www.sandbox.paypal.com/signin',{waitUntil:'domcontentloaded',timeout:45000});
 await pp.locator('#email').fill(buyer[2],{timeout:15000});if(await pp.locator('#btnNext').isVisible())await pp.locator('#btnNext').click();
 await pp.locator('#password').fill(buyer[3],{timeout:15000});await pp.locator('#btnLogin').click();await pp.waitForTimeout(4000);
 const paypalApprove=async url=>{await pp.goto(url,{waitUntil:'domcontentloaded',timeout:45000});await pp.waitForTimeout(3000);
  for(let i=0;i<8;i++){if(new URL(pp.url()).hostname==='localhost')return;const b=pp.getByRole('button',{name:/^(Pay Now|Agree & Pay|Agree and Pay|Complete Purchase|Continue|Agree & Continue|Agree and Continue|Continue to Review Order|Pay)$/i}).first();if(await b.isVisible().catch(()=>false))await b.click();await pp.waitForTimeout(3000);}
  throw new Error('PayPal approval did not return');};
 const shots=[];let n=0;
 // One shot = fresh recorded context; fn(page, mark) performs scroll/dwell/clicks. mark() sets the trim-in point.
 const shot=async(line,url,fn)=>{const ctx=await browser.newContext({viewport:{width:1280,height:800},storageState:state,recordVideo:{dir:out+'/raw',size:{width:1280,height:800}}});
  await ctx.route('**/assets/loader.js',r=>r.fulfill({status:200,contentType:'text/javascript',body:''}));
  const page=await ctx.newPage();const t0=Date.now();await page.goto(url);await page.waitForTimeout(400);let tin=0;
  const mark=()=>{tin=(Date.now()-t0)/1000;};
  const api2={page,mark,
   step:t=>page.locator('li.step',{has:page.locator('h3',{hasText:t})}).first(),
   jump:async(loc,o=-70)=>{await loc.evaluate((el,o)=>window.scrollTo({top:el.getBoundingClientRect().top+window.scrollY+o,behavior:'instant'}),o);await page.waitForTimeout(150);},
   glide:async(loc,o=-70)=>{await loc.evaluate((el,o)=>window.scrollTo({top:el.getBoundingClientRect().top+window.scrollY+o,behavior:'smooth'}),o);await page.waitForTimeout(900);},
   btn:nm=>page.getByRole('button',{name:nm,exact:false}).first()};
  await fn(api2);const tout=(Date.now()-t0)/1000;await page.waitForTimeout(300);const v=page.video();await ctx.close();
  shots.push({line,file:await v.path(),tin,tout,n:n++});};
 try{
  // Story A
  let taskA;
  await shot(0,'http://localhost:3000/agent',async s=>{s.mark();const ta=s.page.locator('textarea[name=request]');await ta.fill('');const st=Date.now();await ta.pressSequentially(REQUEST,{delay:28});const rest=D(0)-(Date.now()-st)-300;if(rest>0)await s.page.waitForTimeout(rest);await s.btn('Run buyer agent').click();await s.page.waitForURL(/\/agent\/[0-9a-f-]{36}$/);taskA=s.page.url().split('/').pop();});
  let a=await waitTask(taskA,x=>!['planning','negotiating'].includes(x.state));if(a.state!=='awaiting_approval')throw new Error('no proposal A');
  const urlA='http://localhost:3000/agent/'+taskA;
  await shot(1,urlA,async s=>{await s.jump(s.step('Understand'));s.mark();await s.page.waitForTimeout(D(1));});
  await shot(2,urlA,async s=>{await s.jump(s.step('Negotiate'));s.mark();await s.page.waitForTimeout(D(2)*0.45);await s.page.evaluate(()=>window.scrollBy({top:560,behavior:'smooth'}));await s.page.waitForTimeout(D(2)*0.55);});
  await shot(3,urlA,async s=>{await s.jump(s.step('Contract'));s.mark();await s.page.waitForTimeout(D(3)-2400);await s.glide(s.step('Buyer approval'),-300);await s.page.waitForTimeout(700);await s.btn('Approve exact contract').click();await s.page.waitForTimeout(500);});
  a=await waitTask(taskA,x=>x.order&&x.order.state==='approval_pending',60000);const oa=a.order;
  await paypalApprove(oa.approval_url);await api('/api/orders/'+oa.id+'/authorize',{});const authA=await waitOrder(oa.id,['authorized'],60000);
  await shot(4,urlA,async s=>{await s.jump(s.step('PayPal authorization'));s.mark();await s.page.waitForTimeout(D(4));});
  await shot(5,urlA,async s=>{await s.jump(s.step('Delivery'));s.mark();await s.page.waitForTimeout(D(5)-900);await s.btn('Deliver report with injected instructions').click();await s.page.waitForTimeout(600);});
  await waitOrder(oa.id,['buyer_review'],150000);
  await shot(6,urlA,async s=>{await s.jump(s.step('Verification'));s.mark();await s.page.waitForTimeout(D(6)*0.35);await s.glide(s.page.locator('text=AI DELIVERY ANALYST').first(),-40);await s.page.waitForTimeout(D(6)*0.65-900);});
  await shot(7,urlA,async s=>{await s.jump(s.step('Settlement'),-200);s.mark();await s.page.waitForTimeout(1600);await s.btn('Reject delivery').click();await s.page.waitForTimeout(400);});
  const finA=await waitOrder(oa.id,['voided','reconciliation_required'],60000);
  await shot(7,urlA,async s=>{await s.jump(s.step('PayPal authorization'));s.mark();await s.page.waitForTimeout(D(7)-2000);});
  // Story B (setup off camera)
  const tb=await api('/api/agent/tasks',{request:REQUEST});let b=await waitTask(tb.id,x=>!['planning','negotiating'].includes(x.state));
  await api('/api/agent/tasks/'+tb.id+'/approve',{proposal_digest:b.proposal_digest});b=await waitTask(tb.id,x=>x.order&&x.order.state==='approval_pending',60000);
  const ob=b.order;await paypalApprove(ob.approval_url);await api('/api/orders/'+ob.id+'/authorize',{});const authB=await waitOrder(ob.id,['authorized'],60000);
  const urlB='http://localhost:3000/agent/'+tb.id;
  await shot(8,urlB,async s=>{await s.jump(s.step('PayPal authorization'));s.mark();await s.page.waitForTimeout(D(8)/2);await s.glide(s.step('Delivery'));await s.page.waitForTimeout(D(8)/2-1500);await s.btn('Deliver compliant report').click();await s.page.waitForTimeout(400);});
  await waitOrder(ob.id,['buyer_review'],150000);
  await shot(9,urlB,async s=>{await s.jump(s.page.locator('text=AI DELIVERY ANALYST').first(),-40);s.mark();await s.page.waitForTimeout(D(9));});
  await shot(10,urlB,async s=>{await s.jump(s.step('Settlement'),-200);s.mark();await s.page.waitForTimeout(1600);await s.btn('Approve delivery').click();await s.page.waitForTimeout(400);});
  const finB=await waitOrder(ob.id,['captured','reconciliation_required'],60000);
  await shot(10,urlB,async s=>{await s.jump(s.step('PayPal authorization'));s.mark();await s.page.waitForTimeout(D(10)-2000);});
  await shot(11,'http://localhost:3000/intelligence',async s=>{await s.page.waitForTimeout(2500);s.mark();await s.page.waitForTimeout(D(11)*0.4);await s.page.getByRole('button',{name:'AI flagged'}).click().catch(()=>{});await s.page.waitForTimeout(D(11)*0.35);await s.page.getByRole('button',{name:'All',exact:true}).click().catch(()=>{});await s.page.waitForTimeout(D(11)*0.25);});
  fs.writeFileSync(out+'/shots.json',JSON.stringify({shots,orders:{a:{task:taskA,order:oa.id,paypal:oa.paypal_order,auth:authA.authorization_id,final:finA.state},b:{task:tb.id,order:ob.id,paypal:ob.paypal_order,auth:authB.authorization_id,final:finB.state,capture:finB.provider_result&&finB.provider_result.id}}},null,1));
  console.log('recorded',shots.length,'shots; A',finA.state,'B',finB.state);
 }catch(e){console.log('stopped:',e.message.split('\n')[0]);process.exitCode=1;}
 finally{await browser.close();}
})();
